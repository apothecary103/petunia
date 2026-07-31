use std::collections::HashMap;

use iced::widget::image;
use uuid::Uuid;

use super::{Contact, Group, History, Index, Message, Thread, contact_name};

/// Everything the panes read while rendering, so that adding a buffer kind does
/// not mean threading another parameter through every `view`.
pub struct State {
    pub aci: Uuid,
    pub contacts: Vec<Contact>,
    pub groups: Vec<Group>,
    pub index: Index,
    pub histories: HashMap<Thread, History>,
    pub avatars: HashMap<Thread, image::Handle>,
}

impl State {
    pub fn new(aci: Uuid) -> Self {
        Self {
            aci,
            contacts: Vec::new(),
            groups: Vec::new(),
            index: Index::default(),
            histories: HashMap::new(),
            avatars: HashMap::new(),
        }
    }

    pub fn history(&self, thread: &Thread) -> Option<&History> {
        self.histories.get(thread)
    }

    pub fn history_mut(&mut self, thread: &Thread) -> &mut History {
        self.histories.entry(thread.clone()).or_default()
    }

    pub fn avatar(&self, thread: &Thread) -> Option<&image::Handle> {
        self.avatars.get(thread)
    }

    pub fn title(&self, thread: &Thread) -> String {
        self.index
            .name(thread)
            .map(str::to_string)
            .unwrap_or_else(|| match thread {
                Thread::Contact(contact) => contact.uuid().to_string()[..8].to_string(),
                Thread::Group(_) => "Group".into(),
            })
    }

    pub fn sender_name(&self, sender: Uuid) -> String {
        if sender == self.aci {
            return "You".into();
        }
        contact_name(&self.contacts, sender)
            .map(str::to_string)
            .unwrap_or_else(|| sender.to_string()[..8].to_string())
    }

    pub fn contacts_updated(&mut self, contacts: Vec<Contact>, groups: Vec<Group>) {
        self.index.rebuild(&contacts, &groups, self.aci);
        self.contacts = contacts;
        self.groups = groups;
    }

    pub fn record(&mut self, thread: &Thread, message: &Message) {
        let contacts = &self.contacts;
        self.index.touch(thread, message, || match thread {
            Thread::Contact(contact) => contact_name(contacts, contact.uuid())
                .map(str::to_string)
                .unwrap_or_else(|| contact.uuid().to_string()[..8].to_string()),
            Thread::Group(_) => "Group".into(),
        });
    }
}
