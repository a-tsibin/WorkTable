use std::sync::Arc;

use derive_more::{Display, Error, From};
#[cfg(feature = "perf_measurements")]
use performance_measurement_codegen::performance_measurement;
use rkyv::ser::serializers::AllocSerializer;
use rkyv::{Archive, Deserialize, Serialize};
use scc::ebr::Guard;
use scc::tree_index::TreeIndex;

use crate::in_memory::page::Link;
use crate::in_memory::{DataPages, RowWrapper, StorableRow};
use crate::primary_key::{PrimaryKeyGenerator, TablePrimaryKey};
use crate::{in_memory, TableIndex, TableRow};
use crate::lock::LockMap;
use crate::prelude::ArchivedRow;

#[derive(Debug)]
pub struct WorkTable<Row, Pk, I = (), PkGen = <Pk as TablePrimaryKey>::Generator>
where
    Pk: Clone + Ord + 'static,
    Row: StorableRow,
{
    pub data: DataPages<Row>,

    pub pk_map: TreeIndex<Pk, Link>,

    pub indexes: I,

    pub pk_gen: PkGen,

    pub lock_map: LockMap
}

// Manual implementations to avoid unneeded trait bounds.
impl<Row, Pk, I> Default for WorkTable<Row, Pk, I>
where
    Pk: Clone + Ord + TablePrimaryKey,
    I: Default,
    <Pk as TablePrimaryKey>::Generator: Default,
    Row: StorableRow,
    <Row as StorableRow>::WrappedRow: RowWrapper<Row>,
{
    fn default() -> Self {
        Self {
            data: DataPages::new(),
            pk_map: TreeIndex::new(),
            indexes: I::default(),
            pk_gen: Default::default(),
            lock_map: LockMap::new(),
        }
    }
}

impl<Row, Pk, I, PkGen> WorkTable<Row, Pk, I, PkGen>
where
    Row: TableRow<Pk>,
    Pk: Clone + Ord + TablePrimaryKey,
    Row: StorableRow,
    <Row as StorableRow>::WrappedRow: RowWrapper<Row>,
{
    pub fn get_next_pk(&self) -> Pk
    where PkGen: PrimaryKeyGenerator<Pk>,
    {
        self.pk_gen.next()
    }

    /// Selects `Row` from table identified with provided primary key. Returns `None` if no value presented.
    #[cfg_attr(
        feature = "perf_measurements",
        performance_measurement(prefix_name = "WorkTable")
    )]
    pub fn select(&self, pk: Pk) -> Option<Row>
    where
        Row: Archive,
        <<Row as StorableRow>::WrappedRow as Archive>::Archived: Deserialize<
            <Row as StorableRow>::WrappedRow,
            rkyv::de::deserializers::SharedDeserializeMap,
        >,
    {
        let guard = Guard::new();
        let link = self.pk_map.peek(&pk, &guard)?;
        self.data.select(*link).ok()
    }

    #[cfg_attr(
        feature = "perf_measurements",
        performance_measurement(prefix_name = "WorkTable")
    )]
    pub fn insert<const ROW_SIZE_HINT: usize>(&self, row: Row) -> Result<Pk, WorkTableError>
    where
        Row: Archive + Serialize<AllocSerializer<ROW_SIZE_HINT>> + Clone,
        <Row as StorableRow>::WrappedRow: Archive + Serialize<AllocSerializer<ROW_SIZE_HINT>>,
        Pk: Clone,
        I: TableIndex<Row>,
    {
        let pk = row.get_primary_key().clone();
        let link = self
            .data
            .insert::<ROW_SIZE_HINT>(row.clone())
            .map_err(WorkTableError::PagesError)?;
        self.pk_map
            .insert(pk.clone(), link)
            .map_err(|_| WorkTableError::AlreadyExists)?;
        self.indexes.save_row(row, link)?;

        Ok(pk)
    }
}

#[derive(Debug, Display, Error, From)]
pub enum WorkTableError {
    NotFound,
    AlreadyExists,
    SerializeError,
    PagesError(in_memory::PagesExecutionError),
}

#[cfg(test)]
mod tests {
    use worktable_codegen::worktable;

    use crate::prelude::*;

    worktable! (
        name: Test,
        columns: {
            id: u64 primary_key autoincrement,
            test: i64,
            another: u64,
            exchange: String
        },
        indexes: {
            test_idx: test unique,
            exchnage_idx: exchange,
        }
        queries: {
            update: {
                AnotherByExchange(another) by exchange,
                AnotherByTest(another) by test,
                AnotherById(another) by id,
            }
        }
    );

    mod array {
        use derive_more::From;
        use rkyv::{Archive, Deserialize, Serialize};
        use worktable_codegen::worktable;

        use crate::prelude::*;
        use crate::primary_key::TablePrimaryKey;

        type Arr = [u32; 4];

        worktable! (
            name: Test,
            columns: {
                id: u64 primary_key autoincrement,
                test: Arr
            },
            queries: {
                update: {
                    Test(test) by id,
                }
            }
        );

        #[test]
        fn insert() {
            let table = TestWorkTable::default();
            let row = TestRow {
                id: 1,
                test: [0; 4],
            };
            let pk = table.insert::<{ TestRow::ROW_SIZE }>(row.clone()).unwrap();
            let selected_row = table.select(pk).unwrap();

            assert_eq!(selected_row, row);
            assert!(table.select(2.into()).is_none())
        }

        #[tokio::test]
        async fn update() {
            let table = TestWorkTable::default();
            let row = TestRow {
                id: 1,
                test: [0; 4],
            };
            let pk = table.insert::<{ TestRow::ROW_SIZE }>(row.clone()).unwrap();
            let new_row = TestRow {
                id: 1,
                test: [1; 4]
            };
            table.update::<{ TestRow::ROW_SIZE }>(new_row.clone()).await.unwrap();
            let selected_row = table.select(pk).unwrap();
            assert_eq!(selected_row, new_row);
            assert!(table.select(2.into()).is_none())
        }

        #[tokio::test]
        async fn update_in_a_middle() {
            let table = TestWorkTable::default();
            for i in 0..10 {
                let row = TestRow {
                    id: i,
                    test: [0; 4],
                };
                let _ = table.insert::<{ TestRow::ROW_SIZE }>(row.clone()).unwrap();
            }
            let new_row = TestRow {
                id: 3,
                test: [1; 4]
            };
            table.update::<{ TestRow::ROW_SIZE }>(new_row.clone()).await.unwrap();
            let selected_row = table.select(3.into()).unwrap();
            assert_eq!(selected_row, new_row);
        }
    }

    mod enum_ {
        use derive_more::From;
        use rkyv::{Archive, Deserialize, Serialize};
        use worktable_codegen::worktable;

        use crate::prelude::*;
        use crate::primary_key::TablePrimaryKey;

        #[derive(Archive, Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
        #[archive(compare(PartialEq))]
        #[archive_attr(derive(Debug))]
        pub enum SomeEnum {
            First,
            Second,
            Third,
        }

        worktable! (
            name: Test,
            columns: {
                id: u64 primary_key autoincrement,
                test: SomeEnum
            }
        );

        #[test]
        fn insert() {
            let table = TestWorkTable::default();
            let row = TestRow {
                id: 1,
                test: SomeEnum::First,
            };
            let pk = table.insert::<{ crate::table::tests::tuple_primary_key::TestRow::ROW_SIZE }>(row.clone()).unwrap();
            let selected_row = table.select(pk).unwrap();

            assert_eq!(selected_row, row);
            assert!(table.select(2.into()).is_none())
        }
    }

    mod spawn {
        use std::sync::Arc;
        use super::*;

        #[tokio::test]
        async fn update() {
            let table = Arc::new(TestWorkTable::default());
            let row = TestRow {
                id: table.get_next_pk().into(),
                test: 1,
                another: 1,
                exchange: "test".to_string(),
            };
            let pk = table.insert::<{ TestRow::ROW_SIZE }>(row.clone()).unwrap();
            let updated = TestRow {
                id: pk.clone().into(),
                test: 2,
                another: 3,
                exchange: "test".to_string(),
            };
            let shared = table.clone();
            let shared_updated = updated.clone();
            tokio::spawn(async move { shared.update::<{ TestRow::ROW_SIZE }>(shared_updated).await }).await.unwrap().unwrap();
            let selected_row = table.select(pk).unwrap();

            assert_eq!(selected_row, updated);
            assert!(table.select(2.into()).is_none())
        }
    }

    mod custom_pk {
        use std::sync::atomic::{AtomicU64, Ordering};

        use derive_more::From;
        use rkyv::{Archive, Deserialize, Serialize};
        use worktable_codegen::worktable;

        use crate::prelude::*;
        use crate::primary_key::TablePrimaryKey;

        #[derive(Archive, Debug, Default, Deserialize, Clone, Eq, From, PartialOrd, PartialEq, Ord, Serialize)]
        #[archive(compare(PartialEq))]
        #[archive_attr(derive(Debug))]
        struct CustomId(u64);

        #[derive(Debug, Default)]
        pub struct Generator(AtomicU64);

        impl PrimaryKeyGenerator<TestPrimaryKey> for Generator {
            fn next(&self) -> TestPrimaryKey {
                let res = self.0.fetch_add(1, Ordering::Relaxed);

                if res >= 10 {
                    self.0.store(0, Ordering::Relaxed);
                }

                CustomId::from(res).into()
            }
        }

        impl TablePrimaryKey for TestPrimaryKey {
            type Generator = Generator;
        }

        worktable! (
            name: Test,
            columns: {
                id: CustomId primary_key custom,
                test: u64
            }
        );

        #[test]
        fn test_custom_pk() {
            let table = TestWorkTable::default();
            let pk = table.get_next_pk();
            assert_eq!(pk, CustomId::from(0).into());

            for _ in 0..10 {
                let _ = table.get_next_pk();
            }
            let pk = table.get_next_pk();
            assert_eq!(pk, CustomId::from(0).into());
        }
    }

    mod tuple_primary_key {
        use worktable_codegen::worktable;

        use crate::prelude::*;

        worktable! (
            name: Test,
            columns: {
                id: u64 primary_key,
                test: u64 primary_key,
                another: i64,
            }
        );

        #[test]
        fn insert() {
            let table = TestWorkTable::default();
            let row = TestRow {
                id: 1,
                test: 1,
                another: 1,
            };
            let pk = table.insert::<{ TestRow::ROW_SIZE }>(row.clone()).unwrap();
            let selected_row = table.select(pk).unwrap();

            assert_eq!(selected_row, row);
            assert!(table.select((1, 0).into()).is_none())
        }
    }

    mod uuid {
        use uuid::Uuid;
        use worktable_codegen::worktable;

        use crate::prelude::*;

        worktable! (
            name: Test,
            columns: {
                id: Uuid primary_key,
                another: i64,
            }
        );

        #[test]
        fn insert() {
            let table = TestWorkTable::default();
            let row = TestRow {
                id: Uuid::new_v4(),
                another: 1,
            };
            let pk = table.insert::<{ TestRow::ROW_SIZE }>(row.clone()).unwrap();
            let selected_row = table.select(pk).unwrap();

            assert_eq!(selected_row, row);
            assert!(table.select(Uuid::new_v4().into()).is_none())
        }
    }

    // mod eyre {
    //     use eyre::*;
    //     use worktable_codegen::worktable;
    //
    //     use crate::prelude::*;
    //
    //     worktable! (
    //         name: Test,
    //         columns: {
    //             id: u64 primary_key,
    //             test: u64
    //         }
    //     );
    //
    //     #[test]
    //     fn test() {
    //         let table = TestWorkTable::default();
    //         let row = TestRow {
    //             id: 1,
    //             test: 1,
    //         };
    //         let pk = table.insert::<{ crate::table::tests::tuple_primary_key::TestRow::ROW_SIZE }>(row.clone()).unwrap();
    //         let selected_row = table.select(pk).unwrap();
    //
    //         assert_eq!(selected_row, row);
    //         assert!(table.select((1, 0).into()).is_none())
    //     }
    // }

    #[test]
    fn bench() {
        let table = TestWorkTable::default();

        let mut v = Vec::with_capacity(10000);

        for i in 0..10000 {
            let row = TestRow {
                id: table.get_next_pk().into(),
                test: i + 1,
                another: 1,
                exchange: "XD".to_string(),
            };

            let a = table.insert::<24>(row).expect("TODO: panic message");
            v.push(a)
        }

        for a in v {
            table.select(a).expect("TODO: panic message");
        }
    }

    #[test]
    fn insert() {
        let table = TestWorkTable::default();
        let row = TestRow {
            id: table.get_next_pk().into(),
            test: 1,
            another: 1,
            exchange: "test".to_string(),
        };
        let pk = table.insert::<{ TestRow::ROW_SIZE }>(row.clone()).unwrap();
        let selected_row = table.select(pk).unwrap();

        assert_eq!(selected_row, row);
        assert!(table.select(2.into()).is_none())
    }

    #[tokio::test]
    async fn update() {
        let table = TestWorkTable::default();
        let row = TestRow {
            id: table.get_next_pk().into(),
            test: 1,
            another: 1,
            exchange: "test".to_string(),
        };
        let pk = table.insert::<{ TestRow::ROW_SIZE }>(row.clone()).unwrap();
        let updated = TestRow {
            id: pk.clone().into(),
            test: 2,
            another: 3,
            exchange: "test".to_string(),
        };
        table.update::<{ TestRow::ROW_SIZE }>(updated.clone()).await.unwrap();
        let selected_row = table.select(pk).unwrap();

        assert_eq!(selected_row, updated);
        assert!(table.select(2.into()).is_none())
    }

    #[tokio::test]
    async fn delete() {
        let table = TestWorkTable::default();
        let row = TestRow {
            id: table.get_next_pk().into(),
            test: 1,
            another: 1,
            exchange: "test".to_string(),
        };
        let pk = table.insert::<{ TestRow::ROW_SIZE }>(row.clone()).unwrap();
        let guard = Guard::new();
        let link = *table.0.pk_map.peek(&pk, &guard).unwrap();
        table.delete(pk.clone()).await.unwrap();
        let selected_row = table.select(pk);
        assert!(selected_row.is_none());
        let selected_row = table.select_by_test(1);
        assert!(selected_row.is_none());
        let selected_row = table.select_by_exchange("test".to_string()).unwrap();
        assert!(selected_row.is_empty());

        let updated = TestRow {
            id: table.get_next_pk().into(),
            test: 2,
            another: 3,
            exchange: "test".to_string(),
        };
        let pk = table.insert::<{ TestRow::ROW_SIZE }>(updated.clone()).unwrap();
        let new_link = *table.0.pk_map.peek(&pk, &guard).unwrap();

        assert_eq!(link, new_link)
    }

    #[tokio::test]
    async fn delete_and_insert_less() {
        let table = TestWorkTable::default();
        let row = TestRow {
            id: table.get_next_pk().into(),
            test: 0,
            another: 0,
            exchange: "test".to_string(),
        };
        let _ = table.insert::<{ TestRow::ROW_SIZE }>(row.clone()).unwrap();
        let row = TestRow {
            id: table.get_next_pk().into(),
            test: 1,
            another: 1,
            exchange: "test1234567890".to_string(),
        };
        let pk = table.insert::<{ TestRow::ROW_SIZE }>(row.clone()).unwrap();
        let guard = Guard::new();
        let link = *table.0.pk_map.peek(&pk, &guard).unwrap();
        table.delete(pk.clone()).await.unwrap();
        let selected_row = table.select(pk);
        assert!(selected_row.is_none());

        let updated = TestRow {
            id: table.get_next_pk().into(),
            test: 2,
            another: 3,
            exchange: "test1".to_string(),
        };
        let pk = table.insert::<{ TestRow::ROW_SIZE }>(updated.clone()).unwrap();
        let new_link = *table.0.pk_map.peek(&pk, &guard).unwrap();

        assert_ne!(link, new_link)
    }

    #[tokio::test]
    async fn delete_and_replace() {
        let table = TestWorkTable::default();
        let row = TestRow {
            id: table.get_next_pk().into(),
            test: 0,
            another: 0,
            exchange: "test1".to_string(),
        };
        let _ = table.insert::<{ TestRow::ROW_SIZE }>(row.clone()).unwrap();
        let row = TestRow {
            id: table.get_next_pk().into(),
            test: 1,
            another: 1,
            exchange: "test".to_string(),
        };
        let pk = table.insert::<{ TestRow::ROW_SIZE }>(row.clone()).unwrap();
        let guard = Guard::new();
        let link = *table.0.pk_map.peek(&pk, &guard).unwrap();
        table.delete(pk.clone()).await.unwrap();
        let selected_row = table.select(pk);
        assert!(selected_row.is_none());

        let updated = TestRow {
            id: table.get_next_pk().into(),
            test: 2,
            another: 3,
            exchange: "test".to_string(),
        };
        let pk = table.insert::<{ TestRow::ROW_SIZE }>(updated.clone()).unwrap();
        let new_link = *table.0.pk_map.peek(&pk, &guard).unwrap();

        assert_eq!(link, new_link)
    }

    #[tokio::test]
    async fn upsert() {
        let table = TestWorkTable::default();
        let row = TestRow {
            id: table.get_next_pk().into(),
            test: 1,
            another: 1,
            exchange: "test".to_string(),
        };
        table.upsert::<{ TestRow::ROW_SIZE }>(row.clone()).await.unwrap();
        let updated = TestRow {
            id: row.id,
            test: 2,
            another: 3,
            exchange: "test".to_string(),
        };
        table.upsert::<{ TestRow::ROW_SIZE }>(updated.clone()).await.unwrap();
        let selected_row = table.select(row.id.into()).unwrap();

        assert_eq!(selected_row, updated);
        assert!(table.select(2.into()).is_none())
    }

    #[test]
    fn insert_same() {
        let table = TestWorkTable::default();
        let row = TestRow {
            id: table.get_next_pk().into(),
            test: 1,
            another: 1,
            exchange: "test".to_string(),
        };
        let _ = table.insert::<{ TestRow::ROW_SIZE }>(row.clone()).unwrap();
        let res = table.insert::<{ TestRow::ROW_SIZE }>(row.clone());
        assert!(res.is_err())
    }

    #[test]
    fn insert_exchange_same() {
        let table = TestWorkTable::default();
        let row = TestRow {
            id: table.get_next_pk().into(),
            test: 1,
            another: 1,
            exchange: "test".to_string(),
        };
        let _ = table.insert::<{ TestRow::ROW_SIZE }>(row.clone()).unwrap();
        let row = TestRow {
            id: table.get_next_pk().into(),
            test: 1,
            another: 1,
            exchange: "test".to_string(),
        };
        let res = table.insert::<{ TestRow::ROW_SIZE }>(row.clone());
        assert!(res.is_err())
    }

    #[test]
    fn select_by_exchange() {
        let table = TestWorkTable::default();
        let row = TestRow {
            id: table.get_next_pk().into(),
            test: 1,
            another: 1,
            exchange: "test".to_string(),
        };
        let _ = table.insert::<{ TestRow::ROW_SIZE }>(row.clone()).unwrap();
        let selected_rows = table.select_by_exchange("test".to_string()).unwrap();

        assert_eq!(selected_rows.len(), 1);
        assert!(selected_rows.contains(&row));
        assert!(table.select_by_exchange("test1".to_string()).is_err())
    }

    #[test]
    fn select_multiple_by_exchange() {
        let table = TestWorkTable::default();
        let row = TestRow {
            id: table.get_next_pk().into(),
            test: 1,
            another: 1,
            exchange: "test".to_string(),
        };
        let _ = table.insert::<{ TestRow::ROW_SIZE }>(row.clone()).unwrap();
        let row_next = TestRow {
            id: table.get_next_pk().into(),
            test: 2,
            another: 1,
            exchange: "test".to_string(),
        };
        let _ = table
            .insert::<{ TestRow::ROW_SIZE }>(row_next.clone())
            .unwrap();
        let selected_rows = table.select_by_exchange("test".to_string()).unwrap();

        assert_eq!(selected_rows.len(), 2);
        assert!(selected_rows.contains(&row));
        assert!(selected_rows.contains(&row_next));
        assert!(table.select_by_exchange("test1".to_string()).is_err())
    }

    #[test]
    fn select_by_test() {
        let table = TestWorkTable::default();
        let row = TestRow {
            id: table.get_next_pk().into(),
            test: 1,
            another: 1,
            exchange: "test".to_string(),
        };
        let _ = table.insert::<{ TestRow::ROW_SIZE }>(row.clone()).unwrap();
        let selected_row = table.select_by_test(1).unwrap();

        assert_eq!(selected_row, row);
        assert!(table.select_by_test(2).is_none())
    }

    #[test]
    fn select_all_test() {
        let table = TestWorkTable::default();
        let row1 = TestRow {
            id: table.get_next_pk().into(),
            test: 1,
            another: 1,
            exchange: "test".to_string(),
        };
        let _ = table.insert::<{ TestRow::ROW_SIZE }>(row1.clone()).unwrap();
        let row2 = TestRow {
            id: table.get_next_pk().into(),
            test: 2,
            another: 1,
            exchange: "test".to_string(),
        };
        let _ = table.insert::<{ TestRow::ROW_SIZE }>(row2.clone()).unwrap();

        let all = table.select_all().unwrap();

        assert_eq!(all.len(), 2);
        assert_eq!(&all[0], &row1);
        assert_eq!(&all[1], &row2)
    }

    #[tokio::test]
    async fn test_update_by_non_unique() {
        let table = TestWorkTable::default();
        let row1 = TestRow {
            id: table.get_next_pk().into(),
            test: 1,
            another: 1,
            exchange: "test".to_string(),
        };
        let _ = table.insert::<{ TestRow::ROW_SIZE }>(row1.clone()).unwrap();
        let row2 = TestRow {
            id: table.get_next_pk().into(),
            test: 2,
            another: 1,
            exchange: "test".to_string(),
        };
        let _ = table.insert::<{ TestRow::ROW_SIZE }>(row2.clone()).unwrap();

        let row = AnotherByExchangeQuery {
            another: 3
        };
        table.update_another_by_exchange(row, "test".to_string()).await.unwrap();

        let all = table.select_all().unwrap();

        assert_eq!(all.len(), 2);
        assert_eq!(&all[0], &TestRow {
            id: 0,
            test: 1,
            another: 3,
            exchange: "test".to_string(),
        });
        assert_eq!(&all[1], &TestRow {
            id: 1,
            test: 2,
            another: 3,
            exchange: "test".to_string(),
        })
    }

    #[tokio::test]
    async fn test_update_by_unique() {
        let table = TestWorkTable::default();
        let row = TestRow {
            id: table.get_next_pk().into(),
            test: 1,
            another: 1,
            exchange: "test".to_string(),
        };
        let _ = table.insert::<{ TestRow::ROW_SIZE }>(row.clone()).unwrap();

        let row = AnotherByTestQuery {
            another: 3
        };
        table.update_another_by_test(row, 1).await.unwrap();

        let row = table.select_by_test(1).unwrap();

        assert_eq!(row, TestRow {
            id: 0,
            test: 1,
            another: 3,
            exchange: "test".to_string(),
        })
    }

    #[tokio::test]
    async fn test_update_by_pk() {
        let table = TestWorkTable::default();
        let row = TestRow {
            id: table.get_next_pk().into(),
            test: 1,
            another: 1,
            exchange: "test".to_string(),
        };
        let pk = table.insert::<{ TestRow::ROW_SIZE }>(row.clone()).unwrap();

        let row = AnotherByIdQuery {
            another: 3
        };
        table.update_another_by_id(row, pk).await.unwrap();

        let row = table.select_by_test(1).unwrap();

        assert_eq!(row, TestRow {
            id: 0,
            test: 1,
            another: 3,
            exchange: "test".to_string(),
        })
    }
}
