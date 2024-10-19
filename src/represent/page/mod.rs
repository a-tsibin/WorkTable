mod data;
mod link;
mod r#type;

use derive_more::{Display, From};
use rkyv::{Archive, Deserialize, Serialize};

pub use link::PageLink;
pub use {data::DataPage, data::ExecutionError as DataExecutionError, data::PAGE_BODY_SIZE};

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
