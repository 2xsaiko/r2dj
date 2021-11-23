use pin_project_lite::pin_project;
use thiserror::Error;

#[macro_export]
macro_rules! sync_proxy {
    (
        $v:vis proxy $name:ident {
            $(
                $fv:vis fn $fn_name:ident ($($p:ident : $pty:ty),* $(,)?) $(-> $rty:ty)?;
            )*
        }
    ) => {
        $crate::paste::paste! {
            $v struct $name {
                pipe: std::sync::Mutex<std::sync::mpsc::Sender< [<$name Message>] >>
            }

            impl $name {
                $v fn channel() -> ($name, [<$name Receiver>]) {
                    let (tx, rx) = std::sync::mpsc::channel(20);

                    (
                        $name { pipe: std::sync::Mutex::new(tx) },
                        rx
                    )
                }
            }
        }

        impl $name {
            $(
                $fv async fn $fn_name (&self, $($p : $pty),* ) -> $crate::proxy::Result $(< $rty >)? {
                    let (c, h) = $crate::futures::channel::oneshot::channel();

                    $crate::paste::paste! {
                        let msg = [<$name Message>] :: [< $fn_name:camel >] {
                            $($p,)*
                            callback: c.into()
                        };
                    }

                    $crate::futures::SinkExt::send(&mut *self.pipe.lock().unwrap(), msg).await?;

                    Ok(h.await?)
                }
            )*
        }

        $crate::paste::paste! {
            type [<$name Receiver>] = $crate::futures::channel::mpsc::Receiver< [<$name Message>] >;

            #[derive(Debug)]
            $v enum [<$name Message>] {
                $( [< $fn_name:camel >] { $($p : $pty,)* callback: $crate::proxy::Callback $( < $rty > )? } ),*
            }
        }
    };
}

pub type Result<T = (), E = Error> = std::result::Result<T, E>;

pin_project! {
    #[derive(Debug)]
    #[must_use = "this callback must be used to return a value to the caller"]
    pub struct Callback<T = ()> {
        #[pin]
        pipe: oneshot::Sender<T>,
    }
}

impl<T> Callback<T> {
    pub fn send(self, t: T) -> Result<(), T> {
        self.pipe.send(t)
    }
}

impl<T> From<oneshot::Sender<T>> for Callback<T> {
    fn from(pipe: oneshot::Sender<T>) -> Self {
        Callback { pipe }
    }
}

#[derive(Error, Clone, Eq, PartialEq, Debug)]
pub enum Error {
    #[error("{0}")]
    SendError(#[from] mpsc::SendError),
    #[error("{0}")]
    Canceled(#[from] oneshot::Canceled),
}

mod oneshot {
    use std::error::Error;
    use std::fmt;
    use std::sync::{Arc, Condvar, Mutex};

    pub struct Sender<T> {
        shared: Arc<Shared<T>>,
    }

    impl<T> Sender<T> {
        pub fn send(self, v: T) {
            let mut data = self.shared.data.lock().unwrap();
            *data = Some(v);
        }
    }

    impl<T> Drop for Sender<T> {
        fn drop(&mut self) {
            self.shared.cv.notify_one();
        }
    }

    pub struct Receiver<T> {
        shared: Arc<Shared<T>>,
    }

    impl <T> Receiver<T> {
        pub fn wait_for(self) -> Result<T, Canceled> {
            let lr = self.shared.cv.wait(self.shared.data.lock().unwrap()).unwrap();
        }
    }

    struct Shared<T> {
        data: Mutex<Option<T>>,
        cv: Condvar,
    }

    /// Error returned from a `Receiver<T>` whenever the corresponding `Sender<T>`
    /// is dropped.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub struct Canceled;

    impl fmt::Display for Canceled {
        fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
            write!(fmt, "oneshot canceled")
        }
    }

    impl Error for Canceled {
        fn description(&self) -> &str {
            "oneshot canceled"
        }
    }
}

#[cfg(test)]
mod test {
    use futures::executor::LocalPool;
    use futures::task::{LocalSpawnExt, SpawnExt};
    use futures::StreamExt;

    sync_proxy! {
        pub proxy Test {
            pub fn hello(name: String) -> String;

            pub fn yeah() -> bool;
        }
    }

    async fn run(mut rx: TestReceiver) {
        let mut state = false;

        while let Some(v) = rx.next().await {
            match v {
                TestMessage::Hello { name, callback } => {
                    let result = format!("Hello, {}!", name);
                    let _ = callback.send(result);
                }
                TestMessage::Yeah { callback } => {
                    let _ = callback.send(state);
                    state = !state;
                }
            }
        }
    }

    #[test]
    fn test() {
        let mut pool = LocalPool::new();
        let spawner = pool.spawner();

        pool.spawner()
            .spawn_local(async move {
                let (test, tr) = Test::channel();

                spawner.spawn(run(tr)).unwrap();

                let result = test.hello("2xsaiko".to_string()).await.unwrap();
                assert_eq!("Hello, 2xsaiko!", result);
                assert_eq!(false, test.yeah().await.unwrap());
                assert_eq!(true, test.yeah().await.unwrap());
            })
            .unwrap();

        pool.run();
    }
}
