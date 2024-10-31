use std::marker::PhantomData;
use std::pin::Pin;

use derive_more::{Display, Error};

use crate::page::link::PageLink;

use innodb::page::data::DATA_PAGE_BODY_SIZE;
use innodb::page::{PageId, PAGE_SIZE};
#[cfg(feature = "perf_measurements")]
use performance_measurement_codegen::performance_measurement;
use rkyv::ser::serializers::AllocSerializer;
use rkyv::{Archive, Deserialize, Serialize};

pub struct DataPage<Row> {
    page: innodb::page::data::DataPage,
    phantom: PhantomData<Row>,
}

unsafe impl<Row> Sync for DataPage<Row> {}

impl<Row> DataPage<Row> {
    /// Creates new [`DataPage`] page.
    pub fn new(id: PageId) -> Self {
        let mut page = innodb::page::data::DataPage::new();
        page.header_mut().page_id = id;
        Self {
            page,
            phantom: Default::default(),
        }
    }

    #[cfg_attr(
        feature = "perf_measurements",
        performance_measurement(prefix_name = "DataRow")
    )]
    pub fn save_row<const N: usize>(&mut self, row: &Row) -> Result<PageLink, DataExecutionError>
    where
        Row: Archive + Serialize<AllocSerializer<N>>,
    {
        let bytes = rkyv::to_bytes(row).map_err(|_| DataExecutionError::SerializeError)?;
        let length = bytes.len() as u32;
        let offset = &mut self.page.data_header_mut().offset;
        if *offset + length > DATA_PAGE_BODY_SIZE as _ {
            return Err(DataExecutionError::PageIsFull {
                need: length,
                left: PAGE_SIZE as i64 - *offset as i64,
            });
        }
        let offset0 = *offset;
        *offset += length;

        let inner_data = self.page.body_mut();
        inner_data[offset0 as usize..][..length as usize].copy_from_slice(bytes.as_slice());

        let link = PageLink {
            page_id: self.page.header().page_id,
            offset: offset0,
            length,
        };

        Ok(link)
    }

    #[cfg_attr(
        feature = "perf_measurements",
        performance_measurement(prefix_name = "DataRow")
    )]
    pub unsafe fn save_row_by_link<const N: usize>(
        &mut self,
        row: &Row,
        link: PageLink,
    ) -> Result<PageLink, DataExecutionError>
    where
        Row: Archive + Serialize<AllocSerializer<N>>,
    {
        let bytes = rkyv::to_bytes(row).map_err(|_| DataExecutionError::SerializeError)?;
        let length = bytes.len() as u32;
        if length != link.length {
            return Err(DataExecutionError::InvalidLink);
        }

        let inner_data = self.page.body_mut();
        inner_data[link.offset as usize..][..link.length as usize]
            .copy_from_slice(bytes.as_slice());

        Ok(link)
    }

    pub unsafe fn get_mut_row_ref(
        &mut self,
        link: PageLink,
    ) -> Result<Pin<&mut <Row as Archive>::Archived>, DataExecutionError>
    where
        Row: Archive,
    {
        if link.offset > self.page.data_header().offset {
            return Err(DataExecutionError::DeserializeError);
        }

        let inner_data = self.page.body_mut();
        let bytes = &mut inner_data[link.offset as usize..(link.offset + link.length) as usize];
        Ok(unsafe { rkyv::archived_root_mut::<Row>(Pin::new(&mut bytes[..])) })
    }

    #[cfg_attr(
        feature = "perf_measurements",
        performance_measurement(prefix_name = "DataRow")
    )]
    pub fn get_row_ref(
        &self,
        link: PageLink,
    ) -> Result<&<Row as Archive>::Archived, DataExecutionError>
    where
        Row: Archive,
    {
        if link.offset > self.page.data_header().offset {
            return Err(DataExecutionError::DeserializeError);
        }

        let inner_data = self.page.body();
        let bytes = &inner_data[link.offset as usize..(link.offset + link.length) as usize];
        Ok(unsafe { rkyv::archived_root::<Row>(bytes) })
    }

    #[cfg_attr(
        feature = "perf_measurements",
        performance_measurement(prefix_name = "DataRow")
    )]
    pub fn get_row(&self, link: PageLink) -> Result<Row, DataExecutionError>
    where
        Row: Archive,
        <Row as Archive>::Archived: Deserialize<Row, rkyv::de::deserializers::SharedDeserializeMap>,
    {
        let archived = self.get_row_ref(link)?;
        let mut map = rkyv::de::deserializers::SharedDeserializeMap::new();
        archived
            .deserialize(&mut map)
            .map_err(|_| DataExecutionError::DeserializeError)
    }
}

/// Error that can appear on [`DataPage`] page operations.
#[derive(Copy, Clone, Debug, Display, Error)]
pub enum DataExecutionError {
    /// Error of trying to save row in [`DataPage`] page with not enough space left.
    #[display("need {}, but {} left", need, left)]
    PageIsFull { need: u32, left: i64 },

    /// Error of saving `Row` in [`DataPage`] page.
    SerializeError,

    /// Error of loading `Row` from [`DataPage`] page.
    DeserializeError,

    /// Link provided for saving `Row` is invalid.
    InvalidLink,
}

#[cfg(test)]
mod tests {
    use std::sync::{mpsc, Arc, Mutex};
    use std::thread;

    use crate::page::data::DataPage;
    use innodb::page::data::DATA_PAGE_BODY_SIZE;
    use rkyv::{Archive, Deserialize, Serialize};

    #[derive(
        Archive, Copy, Clone, Deserialize, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
    )]
    #[archive(compare(PartialEq))]
    #[archive_attr(derive(Debug))]
    struct TestRow {
        a: u64,
        b: u64,
    }

    #[test]
    fn data_page_save_row() {
        let mut page = DataPage::<TestRow>::new(1.into());
        let row = TestRow { a: 10, b: 20 };

        let link = page.save_row::<16>(&row).unwrap();
        assert_eq!(link.page_id, page.page.page_id());
        assert_eq!(link.length, 16);
        assert_eq!(link.offset, 0);

        assert_eq!(page.page.data_header().offset, link.length);

        let inner_data = page.page.body();
        let bytes = &inner_data[link.offset as usize..link.length as usize];
        let archived = unsafe { rkyv::archived_root::<TestRow>(bytes) };
        assert_eq!(archived, &row)
    }

    #[test]
    fn data_page_overwrite_row() {
        let mut page = DataPage::<TestRow>::new(1.into());
        let row = TestRow { a: 10, b: 20 };

        let link = page.save_row::<16>(&row).unwrap();

        let new_row = TestRow { a: 20, b: 20 };
        let res = unsafe { page.save_row_by_link::<16>(&new_row, link) }.unwrap();

        assert_eq!(res, link);

        let inner_data = page.page.body();
        let bytes = &inner_data[link.offset as usize..link.length as usize];
        let archived = unsafe { rkyv::archived_root::<TestRow>(bytes) };
        assert_eq!(archived, &new_row)
    }

    #[test]
    fn data_page_full() {
        let mut page = DataPage::<TestRow>::new(1.into());
        page.page.data_header_mut().offset = DATA_PAGE_BODY_SIZE as u32 - 16;
        let row = TestRow { a: 10, b: 20 };
        let _ = page.save_row::<16>(&row).unwrap();

        let new_row = TestRow { a: 20, b: 20 };
        let res = page.save_row::<16>(&new_row);

        assert!(res.is_err());
    }

    #[test]
    fn data_page_full_multithread() {
        let page = DataPage::<TestRow>::new(1.into());
        let shared = Arc::new(Mutex::new(page));

        let (tx, rx) = mpsc::channel();
        let second_shared = shared.clone();

        thread::spawn(move || {
            let mut links = Vec::new();
            for i in 1..10 {
                let row = TestRow {
                    a: 10 + i,
                    b: 20 + i,
                };

                let link = second_shared.lock().unwrap().save_row::<16>(&row);
                links.push(link)
            }

            tx.send(links).unwrap();
        });

        let mut links = Vec::new();
        for i in 1..10 {
            let row = TestRow {
                a: 30 + i,
                b: 40 + i,
            };

            let link = shared.lock().unwrap().save_row::<16>(&row);
            links.push(link)
        }
        let other_links = rx.recv().unwrap();

        print!("{:?}", other_links);
        print!("{:?}", links);
    }

    #[test]
    fn data_page_save_many_rows() {
        let mut page = DataPage::<TestRow>::new(1.into());

        let mut rows = Vec::new();
        let mut links = Vec::new();
        for i in 1..10 {
            let row = TestRow {
                a: 10 + i,
                b: 20 + i,
            };
            rows.push(row);

            let link = page.save_row::<16>(&row);
            links.push(link)
        }

        let inner_data = page.page.body();

        for (i, link) in links.into_iter().enumerate() {
            let link = link.unwrap();

            let bytes = &inner_data[link.offset as usize..(link.offset + link.length) as usize];
            let archived = unsafe { rkyv::archived_root::<TestRow>(bytes) };
            let row = rows.get(i).unwrap();

            assert_eq!(row, archived)
        }
    }

    #[test]
    fn data_page_get_row_ref() {
        let mut page = DataPage::<TestRow>::new(1.into());
        let row = TestRow { a: 10, b: 20 };

        let link = page.save_row::<16>(&row).unwrap();
        let archived = page.get_row_ref(link).unwrap();
        assert_eq!(archived, &row)
    }

    #[test]
    fn data_page_get_row() {
        let mut page = DataPage::<TestRow>::new(1.into());
        let row = TestRow { a: 10, b: 20 };

        let link = page.save_row::<16>(&row).unwrap();
        let deserialized = page.get_row(link).unwrap();
        assert_eq!(deserialized, row)
    }

    #[test]
    fn multithread() {
        let page = DataPage::<TestRow>::new(1.into());
        let shared = Arc::new(Mutex::new(page));

        let (tx, rx) = mpsc::channel();
        let second_shared = shared.clone();

        thread::spawn(move || {
            let mut links = Vec::new();
            for i in 1..10 {
                let row = TestRow {
                    a: 10 + i,
                    b: 20 + i,
                };

                let link = second_shared.lock().unwrap().save_row::<16>(&row);
                links.push(link)
            }

            tx.send(links).unwrap();
        });

        let mut links = Vec::new();
        for i in 1..10 {
            let row = TestRow {
                a: 30 + i,
                b: 40 + i,
            };

            let link = shared.lock().unwrap().save_row::<16>(&row);
            links.push(link)
        }
        let other_links = rx.recv().unwrap();

        let links = other_links
            .into_iter()
            .chain(links.into_iter())
            .map(|v| v.unwrap())
            .collect::<Vec<_>>();

        for link in links {
            let _ = shared.lock().unwrap().get_row(link).unwrap();
        }
    }
}
