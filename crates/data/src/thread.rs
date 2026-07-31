use presage::libsignal_service::protocol::ServiceId;
use presage::store::Thread as PresageThread;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ContactId {
    Aci(Uuid),
    Pni(Uuid),
}

impl ContactId {
    pub fn uuid(&self) -> Uuid {
        match self {
            Self::Aci(uuid) | Self::Pni(uuid) => *uuid,
        }
    }
}

impl From<&ServiceId> for ContactId {
    fn from(service_id: &ServiceId) -> Self {
        match service_id {
            ServiceId::Aci(_) => Self::Aci(service_id.raw_uuid()),
            ServiceId::Pni(_) => Self::Pni(service_id.raw_uuid()),
        }
    }
}

impl From<&ContactId> for ServiceId {
    fn from(contact: &ContactId) -> Self {
        match contact {
            ContactId::Aci(uuid) => Self::Aci((*uuid).into()),
            ContactId::Pni(uuid) => Self::Pni((*uuid).into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Thread {
    Contact(ContactId),
    Group([u8; 32]),
}

impl Thread {
    /// Stable bytes identifying this thread, for a per-thread colour and for
    /// element ids that must survive a re-render.
    pub fn seed(&self) -> &[u8] {
        match self {
            Self::Contact(ContactId::Aci(uuid) | ContactId::Pni(uuid)) => uuid.as_bytes(),
            Self::Group(master_key) => master_key,
        }
    }

    pub fn is_group(&self) -> bool {
        matches!(self, Self::Group(_))
    }
}

impl From<&PresageThread> for Thread {
    fn from(thread: &PresageThread) -> Self {
        match thread {
            PresageThread::Contact(service_id) => Self::Contact(service_id.into()),
            PresageThread::Group(master_key) => Self::Group(*master_key),
        }
    }
}

impl From<&Thread> for PresageThread {
    fn from(thread: &Thread) -> Self {
        match thread {
            Thread::Contact(contact) => Self::Contact(contact.into()),
            Thread::Group(master_key) => Self::Group(*master_key),
        }
    }
}
