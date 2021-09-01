use crate::server_state::{ChannelRef, UserRef};

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Event {
    Message(Message),
    UserMoved(UserMoved),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Message {
    pub actor: Option<UserRef>,
    pub receivers: Vec<UserRef>,
    pub channels: Vec<ChannelRef>,
    pub message: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct UserMoved {
    pub user: UserRef,
    pub old_channel: ChannelRef,
    pub new_channel: ChannelRef,
}