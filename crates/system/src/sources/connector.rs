//! One connection, made when first asked for and shared from there: a small actor owning it.

use std::future::Future;
use tokio::sync::{mpsc, oneshot};

enum Request<T, E> {
    Get(oneshot::Sender<Result<T, E>>),
    Reset,
}

/// Hands out clones of a `T` that is made on first use. Askers wait in line while it is being made,
/// so there is only ever one; when making it fails, the next asker tries again.
pub struct Connector<T, E>(mpsc::UnboundedSender<Request<T, E>>);

impl<T, E> Clone for Connector<T, E> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T, E> Connector<T, E>
where
    T: Clone + Send + 'static,
    E: Send + 'static,
{
    /// Start the actor on `rt`; it ends when the last `Connector` is dropped. Nothing is connected
    /// until the first `get`, so a source that stays dormant costs nothing.
    pub fn new<F, Fut>(rt: &tokio::runtime::Handle, mut connect: F) -> Self
    where
        F: FnMut() -> Fut + Send + 'static,
        Fut: Future<Output = Result<T, E>> + Send,
    {
        let (tx, mut rx) = mpsc::unbounded_channel();
        rt.spawn(async move {
            let mut current: Option<T> = None;
            while let Some(request) = rx.recv().await {
                match request {
                    Request::Reset => current = None,
                    Request::Get(reply) => {
                        if let Some(c) = &current {
                            let _ = reply.send(Ok(c.clone()));
                            continue;
                        }
                        let made = connect().await;
                        if let Ok(c) = &made {
                            current = Some(c.clone());
                        }
                        let _ = reply.send(made);
                    }
                }
            }
        });
        Self(tx)
    }

    /// The connection, made now if there is none.
    pub async fn get(&self) -> Result<T, E> {
        let (reply, answer) = oneshot::channel();
        let _ = self.0.send(Request::Get(reply));
        match answer.await {
            Ok(result) => result,
            // The runtime is going down, and this task with it.
            Err(_) => std::future::pending().await,
        }
    }

    /// Forget the connection (it failed), so the next `get` makes a new one.
    pub fn reset(&self) {
        let _ = self.0.send(Request::Reset);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::watch;

    /// A connector whose connections are numbers counting up; `fail` makes the next ones fail.
    fn counting() -> (
        Connector<u32, &'static str>,
        watch::Sender<bool>,
        watch::Receiver<u32>,
    ) {
        let (fail, failing) = watch::channel(false);
        let (made, count) = watch::channel(0u32);
        let connector = Connector::new(&tokio::runtime::Handle::current(), move || {
            let failing = *failing.borrow();
            let made = made.clone();
            async move {
                if failing {
                    return Err("no");
                }
                made.send_modify(|n| *n += 1);
                Ok(*made.borrow())
            }
        });
        (connector, fail, count)
    }

    #[tokio::test]
    async fn connects_once_and_shares() {
        let (c, _fail, count) = counting();
        assert_eq!(c.get().await, Ok(1));
        assert_eq!(c.get().await, Ok(1));
        assert_eq!(*count.borrow(), 1);
    }

    #[tokio::test]
    async fn askers_at_once_get_one_connection() {
        let (c, _fail, count) = counting();
        let (a, b, d) = tokio::join!(c.get(), c.get(), c.get());
        assert_eq!((a, b, d), (Ok(1), Ok(1), Ok(1)));
        assert_eq!(*count.borrow(), 1);
    }

    #[tokio::test]
    async fn a_failure_is_tried_again() {
        let (c, fail, _count) = counting();
        fail.send_replace(true);
        assert_eq!(c.get().await, Err("no"));
        fail.send_replace(false);
        assert_eq!(c.get().await, Ok(1));
    }

    #[tokio::test]
    async fn reset_makes_a_new_one() {
        let (c, _fail, _count) = counting();
        assert_eq!(c.get().await, Ok(1));
        c.reset();
        assert_eq!(c.get().await, Ok(2));
    }

    #[tokio::test]
    async fn nothing_is_made_until_asked() {
        let (_c, _fail, count) = counting();
        tokio::task::yield_now().await;
        assert_eq!(*count.borrow(), 0);
    }
}
