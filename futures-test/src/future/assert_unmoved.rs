use futures_core::future::Future;
use futures_core::task::{Context, Poll};
use pin_project::{pin_project, pinned_drop};
use std::pin::Pin;
use std::ptr;
use std::thread::panicking;

/// Combinator for the
/// [`FutureTestExt::assert_unmoved`](super::FutureTestExt::assert_unmoved)
/// method.
#[pin_project(PinnedDrop, !Unpin)]
#[derive(Debug, Clone)]
#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct AssertUnmoved<Fut> {
    #[pin]
    future: Fut,
    this_ptr: *const AssertUnmoved<Fut>,
}

// Safety: having a raw pointer in a struct makes it `!Send`, however the
// pointer is never dereferenced so this is safe.
unsafe impl<Fut: Send> Send for AssertUnmoved<Fut> {}
unsafe impl<Fut: Sync> Sync for AssertUnmoved<Fut> {}

impl<Fut> AssertUnmoved<Fut> {
    pub(super) fn new(future: Fut) -> Self {
        Self {
            future,
            this_ptr: ptr::null(),
        }
    }
}

impl<Fut: Future> Future for AssertUnmoved<Fut> {
    type Output = Fut::Output;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let cur_this = &*self as *const Self;
        if self.this_ptr.is_null() {
            // First time being polled
            *self.as_mut().project().this_ptr = cur_this;
        } else {
            assert_eq!(self.this_ptr, cur_this, "Future moved between poll calls");
        }
        self.project().future.poll(cx)
    }
}

#[pinned_drop]
impl<Fut> PinnedDrop for AssertUnmoved<Fut> {
    fn drop(self: Pin<&mut Self>) {
        // If the thread is panicking then we can't panic again as that will
        // cause the process to be aborted.
        if !panicking() && !self.this_ptr.is_null() {
            let cur_this = &*self as *const Self;
            assert_eq!(self.this_ptr, cur_this, "Future moved before drop");
        }
    }
}

#[cfg(test)]
mod tests {
    use futures_core::future::Future;
    use futures_core::task::{Context, Poll};
    use futures_util::future::pending;
    use futures_util::task::noop_waker;
    use std::pin::Pin;

    use super::AssertUnmoved;

    #[test]
    fn assert_send_sync() {
        fn assert<T: Send + Sync>() {}
        assert::<AssertUnmoved<()>>();
    }

    #[test]
    fn dont_panic_when_not_polled() {
        // This shouldn't panic.
        let future = AssertUnmoved::new(pending::<()>());
        drop(future);
    }

    #[test]
    #[should_panic(expected = "Future moved between poll calls")]
    fn dont_double_panic() {
        // This test should only panic, not abort the process.
        let waker = noop_waker();
        let mut cx = Context::from_waker(&waker);

        // First we allocate the future on the stack and poll it.
        let mut future = AssertUnmoved::new(pending::<()>());
        let pinned_future = unsafe { Pin::new_unchecked(&mut future) };
        assert_eq!(pinned_future.poll(&mut cx), Poll::Pending);

        // Next we move it back to the heap and poll it again. This second call
        // should panic (as the future is moved), but we shouldn't panic again
        // whilst dropping `AssertUnmoved`.
        let mut future = Box::new(future);
        let pinned_boxed_future = unsafe { Pin::new_unchecked(&mut *future) };
        assert_eq!(pinned_boxed_future.poll(&mut cx), Poll::Pending);
    }
}
