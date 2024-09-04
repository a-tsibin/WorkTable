mod set;

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};

use futures::task::AtomicWaker;
use rkyv::{Archive, Deserialize, Serialize};

pub use set::LockMap;

#[derive(Archive, Clone, Copy, Deserialize, Debug, Eq, Hash, Ord, Serialize, PartialEq, PartialOrd)]
pub struct LockId(u16);

#[derive(Debug)]
pub struct Lock {
    id: LockId,
    locked: AtomicBool,
    waker: AtomicWaker
}

impl Future for Lock {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.as_ref().waker.register(cx.waker());
        if self.locked.load(Ordering::Relaxed) {
            Poll::Pending
        } else {
            Poll::Ready(())
        }
    }
}

impl Lock {
    pub fn new(id: LockId) -> Self {
        Self {
            id,
            locked: AtomicBool::from(true),
            waker: AtomicWaker::new(),
        }
    }

    pub fn unlock(&self) {
        self.waker.wake()
    }
}