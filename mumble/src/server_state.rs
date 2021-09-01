use std::collections::HashMap;

use bit_set::BitSet;
use mumble_protocol::control::msgs;
use tokio::sync::broadcast;

use crate::Event;
use crate::event::UserMoved;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct ChannelRef {
    id: u32,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct UserRef {
    id: u32,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Channel {
    id: u32,
    name: String,
    parent: ChannelRef,
    links: BitSet,
    description: String,
    max_users: u32,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct User {
    id: u32,
    name: String,
    registered_id: Option<u32>,
    channel: ChannelRef,
}

#[derive(Debug)]
pub struct ServerState {
    channels: HashMap<u32, Channel>,
    users: HashMap<u32, User>,
    max_message_length: Option<u32>,
    event_subscriber: broadcast::Sender<Event>,
}

impl ChannelRef {
    pub const fn new(id: u32) -> Self {
        ChannelRef { id }
    }

    pub const fn root() -> Self {
        ChannelRef { id: 0 }
    }

    pub fn get(&self, st: &ServerState) -> Option<Channel> {
        st.channels.get(&self.id).cloned()
    }

    pub fn id(&self) -> u32 {
        self.id
    }
}

impl UserRef {
    pub const fn new(id: u32) -> Self {
        UserRef { id }
    }

    pub fn get(&self, st: &ServerState) -> Option<User> {
        st.users.get(&self.id).cloned()
    }

    pub fn session_id(&self) -> u32 {
        self.id
    }
}

impl Channel {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn id(&self) -> u32 {
        self.id
    }

    pub fn parent(&self) -> ChannelRef {
        self.parent
    }

    pub fn links(&self) -> &BitSet {
        &self.links
    }

    pub fn description(&self) -> &str {
        &self.description
    }

    pub fn max_users(&self) -> Option<u32> {
        if self.max_users != 0 {
            Some(self.max_users)
        } else {
            None
        }
    }
}

impl User {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn id(&self) -> u32 {
        self.id
    }

    pub fn registered_id(&self) -> Option<u32> {
        self.registered_id
    }

    pub fn channel(&self) -> ChannelRef {
        self.channel
    }

    pub fn to_ref(&self) -> UserRef {
        UserRef::new(self.id)
    }
}

impl ServerState {
    pub fn new(event_subscriber: broadcast::Sender<Event>) -> Self {
        ServerState {
            channels: Default::default(),
            users: Default::default(),
            max_message_length: None,
            event_subscriber,
        }
    }

    pub fn user(&self, id: u32) -> Option<&User> {
        self.users.get(&id)
    }

    pub fn channel(&self, id: u32) -> Option<&Channel> {
        self.channels.get(&id)
    }

    pub fn update_user(&mut self, mut state: msgs::UserState) {
        let session_id = state.get_session();

        let user = self.users.entry(session_id).or_insert_with(|| User {
            id: session_id,
            name: String::new(),
            registered_id: None,
            channel: ChannelRef::new(0),
        });

        if state.has_name() {
            user.name = state.take_name();
        }

        if state.has_user_id() {
            user.registered_id = Some(state.get_user_id());
        }

        if state.has_channel_id() {
            let new = ChannelRef::new(state.get_channel_id());
            if user.channel != new {
                let _ = self.event_subscriber.send(Event::UserMoved(UserMoved {
                    user: user.to_ref(),
                    old_channel: user.channel,
                    new_channel: new,
                }));
                user.channel = new;
            }
        }
    }

    pub fn max_message_length(&self) -> Option<u32> {
        self.max_message_length
    }

    pub fn remove_user(&mut self, session_id: u32) {
        self.users.remove(&session_id);
    }

    pub fn update_channel(&mut self, mut state: msgs::ChannelState) {
        let channel_id = state.get_channel_id();

        let channel = self.channels.entry(channel_id).or_insert_with(|| Channel {
            id: channel_id,
            name: String::new(),
            parent: ChannelRef::root(),
            links: BitSet::new(),
            description: String::new(),
            max_users: 0,
        });

        if state.has_name() {
            channel.name = state.take_name();
        }

        if state.has_parent() {
            channel.parent = ChannelRef::new(state.get_parent());
        }

        if !state.get_links().is_empty() {
            channel.links.clear();
            channel
                .links
                .extend(state.get_links().iter().map(|v| *v as usize));
        }

        channel
            .links
            .extend(state.get_links_add().iter().map(|v| *v as usize));
        for el in state.get_links_remove() {
            channel.links.remove(*el as usize);
        }

        if state.has_description() {
            channel.description = state.take_description();
        }

        if state.has_max_users() {
            channel.max_users = state.get_max_users();
        }
    }

    pub fn remove_channel(&mut self, channel_id: u32) {
        self.channels.remove(&channel_id);
    }

    pub fn update_server_config(&mut self, config: msgs::ServerConfig) {
        if config.has_message_length() {
            self.max_message_length = Some(config.get_message_length());
        }
    }
}
