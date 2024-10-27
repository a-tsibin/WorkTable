use derive_more::{Display, From};
use rkyv::{Archive, Deserialize, Serialize};

pub mod data;
pub mod link;
pub mod pager;
pub mod row;
pub mod r#type;

/// Represents page's identifier. Is unique within the table bounds
#[derive(
    Archive,
    Copy,
    Clone,
    Deserialize,
    Debug,
    Display,
    Eq,
    From,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
    Serialize,
)]
pub struct PageId(u32);

impl From<PageId> for usize {
    fn from(value: PageId) -> Self {
        value.0 as usize
    }
}
