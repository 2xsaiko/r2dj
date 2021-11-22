use std::convert::TryInto;
use std::io::Cursor;
use std::path::Path;

use async_std::net::TcpStream;
use async_tls::client::TlsStream;
use async_tls::TlsConnector;
use log::{debug, error, info};
use mumble_protocol::control::{msgs, ControlPacket};
use mumble_protocol::crypt::ClientCryptState;
use mumble_protocol::Clientbound;
use rustls::{Certificate, ClientConfig, PrivateKey};
use thiserror::Error;

use crate::server_state::ServerState;

pub async fn connect(
    domain: &str,
    port: u16,
    certfile: Option<impl AsRef<Path>>,
) -> Result<TlsStream<TcpStream>, ConnectError> {
    let mut config = ClientConfig::new();
    config
        .root_store
        .add_server_trust_anchors(&webpki_roots::TLS_SERVER_ROOTS);

    if let Some(certfile) = certfile {
        let certfile = certfile.as_ref();
        let content = async_std::fs::read(certfile).await.unwrap();
        let mut cursor = Cursor::new(&content);
        let certs = rustls_pemfile::certs(&mut cursor).unwrap().into_iter().map(Certificate).collect();
        let mut cursor = Cursor::new(&content);
        let pk = rustls_pemfile::pkcs8_private_keys(&mut cursor).unwrap().into_iter().map(PrivateKey).next().unwrap();

        config.set_single_client_cert(certs, pk).unwrap();
    }

    let stream = TcpStream::connect(format!("{}:{}", domain, port)).await?;
    let connector = TlsConnector::from(config);
    Ok(connector.connect(domain, stream).await?)
}

#[derive(Default)]
pub struct HandshakeState {
    crypt_state: Option<ClientCryptState>,
}

pub enum ResultAction {
    Continue(HandshakeState),
    Disconnect,
    TransferConnected(ClientCryptState, u32),
}

pub async fn handle_packet(
    mut state: HandshakeState,
    server_state: &mut ServerState,
    packet: ControlPacket<Clientbound>,
) -> ResultAction {
    match packet {
        ControlPacket::Ping(msg) => {
            debug!("Pong! {:?}", msg);

            ResultAction::Continue(state)
        }
        ControlPacket::Reject(msg) => {
            error!(
                "Connection rejected by server: {:?} {}",
                msg.get_field_type(),
                msg.get_reason()
            );

            ResultAction::Disconnect
        }
        ControlPacket::Version(msg) => {
            info!("Server is using {:?}", msg);

            ResultAction::Continue(state)
        }
        ControlPacket::ServerSync(msg) => match state.crypt_state {
            Some(crypt_state) => {
                let session = msg.get_session();
                let max_bandwidth = msg.get_max_bandwidth();
                let welcome_text = msg.get_welcome_text();
                let permissions = msg.get_permissions();

                info!("Server says: {}", welcome_text);
                info!(
                    "session id {}, max bandwidth {}, permissions {:X}",
                    session, max_bandwidth, permissions
                );

                ResultAction::TransferConnected(crypt_state, session)
            }
            _ => {
                error!("Server didn't give us crypt setup information during handshake!");

                ResultAction::Disconnect
            }
        },
        ControlPacket::CryptSetup(msg) => match handle_crypt_setup(&msg) {
            Ok(cs) => {
                state.crypt_state = Some(cs);

                ResultAction::Continue(state)
            }
            Err(e) => {
                error!("Error setting up crypt state: {:?}", e);
                ResultAction::Disconnect
            }
        },
        ControlPacket::UserState(p) => {
            server_state.update_user(*p);
            ResultAction::Continue(state)
        }
        ControlPacket::UserRemove(p) => {
            server_state.remove_user(p.get_session());
            ResultAction::Continue(state)
        }
        ControlPacket::ChannelState(p) => {
            server_state.update_channel(*p);
            ResultAction::Continue(state)
        }
        ControlPacket::ChannelRemove(p) => {
            server_state.remove_channel(p.get_channel_id());
            ResultAction::Continue(state)
        }
        x => {
            debug!("Unhandled packet: {:?}", x);

            ResultAction::Continue(state)
        }
    }
}

fn handle_crypt_setup(msg: &msgs::CryptSetup) -> Result<ClientCryptState, CryptSetupError> {
    let key = msg
        .get_key()
        .try_into()
        .map_err(|_| CryptSetupError::InvalidKeySize)?;
    let encrypt_nonce = msg
        .get_client_nonce()
        .try_into()
        .map_err(|_| CryptSetupError::InvalidClientNonceSize)?;
    let decrypt_nonce = msg
        .get_server_nonce()
        .try_into()
        .map_err(|_| CryptSetupError::InvalidServerNonceSize)?;

    Ok(ClientCryptState::new_from(
        key,
        encrypt_nonce,
        decrypt_nonce,
    ))
}

#[derive(Debug, Error)]
pub enum ConnectError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Error)]
enum CryptSetupError {
    #[error("Invalid key size")]
    InvalidKeySize,
    #[error("Invalid client nonce size")]
    InvalidClientNonceSize,
    #[error("Invalid server nonce size")]
    InvalidServerNonceSize,
}
