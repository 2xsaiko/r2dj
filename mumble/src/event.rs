use crate::server_state::{ChannelRef, UserRef};

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Event {
    Message {
        actor: Option<UserRef>,
        receivers: Vec<UserRef>,
        channels: Vec<ChannelRef>,
        message: String,
    },
    UserMoved {
        user: UserRef,
        old_channel: ChannelRef,
        new_channel: ChannelRef,
    },
}
