use std::convert::TryInto;
use std::sync::Arc;

use log::{debug, error, info};
use mumble_protocol::control::{msgs, ControlPacket};
use mumble_protocol::crypt::ClientCryptState;
use mumble_protocol::Clientbound;
use thiserror::Error;
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;
use tokio_rustls::rustls::ClientConfig;
use tokio_rustls::webpki::DNSNameRef;
use tokio_rustls::TlsConnector;

pub async fn connect(domain: &str, ip: u16) -> Result<TlsStream<TcpStream>, ConnectError> {
    let mut config = ClientConfig::new();
    config
        .root_store
        .add_server_trust_anchors(&webpki_roots::TLS_SERVER_ROOTS);
    let stream = TcpStream::connect(format!("{}:{}", domain, ip)).await?;
    let connector = TlsConnector::from(Arc::new(config));
    Ok(connector
        .connect(DNSNameRef::try_from_ascii_str(domain)?, stream)
        .await?)
}

#[derive(Default)]
pub struct HandshakeState {
    crypt_state: Option<ClientCryptState>,
}

pub enum ResultAction {
    Continue(HandshakeState),
    Disconnect,
    TransferConnected(ClientCryptState),
}

pub async fn handle_packet(
    mut state: HandshakeState,
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
                info!("session id {}, max bandwidth {}, permissions {:X}", session, max_bandwidth, permissions);

                ResultAction::TransferConnected(crypt_state)
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
    #[error("Invalid DNS name")]
    Dns(#[from] tokio_rustls::webpki::InvalidDNSNameError),
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
