use crate::page::PageId;
use rkyv::{Archive, Deserialize, Serialize};

pub const LINK_LENGTH: usize = 12;

#[derive(
    Archive, Copy, Clone, Deserialize, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
)]
pub struct PageLink {
    pub page_id: PageId,
    pub offset: u32,
    pub length: u32,
}
static_assertions::const_assert_eq!(size_of::<PageLink>(), LINK_LENGTH);

#[cfg(test)]
mod tests {
    use crate::page::link::{PageLink, LINK_LENGTH};

    #[test]
    fn link_length_valid() {
        let link = PageLink {
            page_id: 1.into(),
            offset: 10,
            length: 20,
        };
        let bytes = rkyv::to_bytes::<_, 16>(&link).unwrap();

        assert_eq!(bytes.len(), LINK_LENGTH)
    }
}
