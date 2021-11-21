use futures::channel::{mpsc, oneshot};
use pin_project_lite::pin_project;
use thiserror::Error;

#[macro_export]
macro_rules! proxy {
    (
        $v:vis proxy $name:ident {
            $(
                $fv:vis async fn $fn_name:ident ($($p:ident : $pty:ty),* $(,)?) $(-> $rty:ty)?;
            )*
        }
    ) => {
        paste::paste! {
            $v struct $name {
                pipe: std::sync::Mutex<futures::channel::mpsc::Sender< [<$name Message>] >>
            }

            impl $name {
                #[allow(unused)]
                $v fn channel() -> ($name, [<$name Receiver>]) {
                    let (tx, rx) = futures::channel::mpsc::channel(20);

                    (
                        $name { pipe: std::sync::Mutex::new(tx) },
                        rx
                    )
                }
            }
        }

        impl $name {
            $(
                #[allow(unused)]
                $fv async fn $fn_name (&self, $($p : $pty),* ) -> $crate::comms::Result $(< $rty >)? {
                    let (c, h) = futures::channel::oneshot::channel();

                    paste::paste! {
                        let msg = [<$name Message>] :: [< $fn_name:camel >] {
                            $($p,)*
                            callback: c.into()
                        };
                    }

                    futures::SinkExt::send(&mut *self.pipe.lock().unwrap(), msg).await?;

                    Ok(h.await?)
                }
            )*
        }

        paste::paste! {
            type [<$name Receiver>] = futures::channel::mpsc::Receiver< [<$name Message>] >;

            #[derive(Debug)]
            #[allow(unused)]
            $v enum [<$name Message>] {
                $( [< $fn_name:camel >] { $($p : $pty,)* callback: $crate::comms::Callback $( < $rty > )? } ),*
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

#[cfg(test)]
mod test {
    use futures::StreamExt;

    proxy! {
        pub proxy Test {
            pub async fn hello(name: String) -> String;

            pub async fn yeah() -> bool;
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
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let (mut test, tr) = Test::channel();

            tokio::spawn(run(tr));

            let result = test.hello("2xsaiko".to_string()).await.unwrap();
            assert_eq!("Hello, 2xsaiko!", result);
            assert_eq!(false, test.yeah().await.unwrap());
            assert_eq!(true, test.yeah().await.unwrap());
        });
    }
}
