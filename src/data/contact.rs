use std::time::Duration;

use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct Contact {
    pub uuid: Uuid,
    pub name: String,
}

impl From<presage::model::contacts::Contact> for Contact {
    fn from(contact: presage::model::contacts::Contact) -> Self {
        Self {
            uuid: contact.uuid,
            name: contact.name,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Group {
    pub master_key: [u8; 32],
    pub title: String,
    pub description: Option<String>,
    pub members: Vec<Member>,
    /// Invited, but not yet joined. They have no name to show yet either: the
    /// invite hides the profile key until it is accepted.
    pub invited: usize,
    /// Waiting for an administrator to let them in.
    pub requesting: usize,
    pub expire_timer: Option<Duration>,
}

/// Someone in a group, with what the group says about them. The label is a real
/// Signal feature — members pick a short phrase and an emoji for themselves —
/// and is not the same thing as their profile name.
#[derive(Debug, Clone)]
pub struct Member {
    pub uuid: Uuid,
    pub role: Role,
    pub label: Option<String>,
    pub label_emoji: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    /// Can change the group, and who is in it.
    Administrator,
    Member,
}

impl Role {
    pub fn label(self) -> Option<&'static str> {
        match self {
            Self::Administrator => Some("Admin"),
            Self::Member => None,
        }
    }
}

impl Member {
    /// The label as it is shown: the emoji and the phrase together, or whichever
    /// of the two the member set.
    pub fn badge(&self) -> Option<String> {
        match (&self.label_emoji, &self.label) {
            (Some(emoji), Some(label)) => Some(format!("{emoji} {label}")),
            (Some(emoji), None) => Some(emoji.clone()),
            (None, Some(label)) => Some(label.clone()),
            (None, None) => None,
        }
    }
}

impl Group {
    /// Members with administrators first and everyone else by name, which is the
    /// order every Signal client lists them in.
    pub fn ordered<'a>(&'a self, name_of: impl Fn(Uuid) -> String + 'a) -> Vec<(&'a Member, String)> {
        let mut listed: Vec<_> = self
            .members
            .iter()
            .map(|member| (member, name_of(member.uuid)))
            .collect();
        listed.sort_by(|(left, left_name), (right, right_name)| {
            left.role
                .cmp(&right.role)
                .then_with(|| left_name.to_lowercase().cmp(&right_name.to_lowercase()))
        });
        listed
    }
}

impl From<(&[u8; 32], presage::model::groups::Group)> for Group {
    fn from((master_key, group): (&[u8; 32], presage::model::groups::Group)) -> Self {
        Self {
            master_key: *master_key,
            title: group.title,
            description: group.description.filter(|text| !text.trim().is_empty()),
            members: group.members.iter().map(Member::from).collect(),
            invited: group.pending_members.len(),
            requesting: group.requesting_members.len(),
            expire_timer: group
                .disappearing_messages_timer
                .map(|timer| Duration::from_secs(u64::from(timer.duration))),
        }
    }
}

impl From<&presage::model::groups::Member> for Member {
    fn from(member: &presage::model::groups::Member) -> Self {
        use presage::libsignal_service::groups_v2::Role as Signal;

        Self {
            uuid: member.aci.into(),
            role: match member.role {
                Signal::Administrator => Role::Administrator,
                _ => Role::Member,
            },
            label: member.label.clone().filter(|label| !label.trim().is_empty()),
            label_emoji: member.label_emoji.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(role: Role, label: Option<&str>, emoji: Option<&str>) -> Member {
        Member {
            uuid: Uuid::new_v4(),
            role,
            label: label.map(str::to_owned),
            label_emoji: emoji.map(str::to_owned),
        }
    }

    #[test]
    fn a_label_reads_as_emoji_then_words() {
        assert_eq!(
            member(Role::Member, Some("on call"), Some("🚨")).badge(),
            Some("🚨 on call".into())
        );
        assert_eq!(
            member(Role::Member, Some("on call"), None).badge(),
            Some("on call".into())
        );
        assert_eq!(
            member(Role::Member, None, Some("🚨")).badge(),
            Some("🚨".into())
        );
        assert_eq!(member(Role::Member, None, None).badge(), None);
    }

    #[test]
    fn only_administrators_carry_a_role_chip() {
        assert_eq!(Role::Administrator.label(), Some("Admin"));
        assert_eq!(Role::Member.label(), None);
    }

    #[test]
    fn members_list_administrators_first_then_by_name() {
        let (admin, zoe, alice) = (
            member(Role::Administrator, None, None),
            member(Role::Member, None, None),
            member(Role::Member, None, None),
        );
        let names = [
            (admin.uuid, "Wendy"),
            (zoe.uuid, "Zoe"),
            (alice.uuid, "alice"),
        ];
        let group = Group {
            master_key: [0; 32],
            title: "Team".into(),
            description: None,
            members: vec![zoe.clone(), alice.clone(), admin.clone()],
            invited: 0,
            requesting: 0,
            expire_timer: None,
        };

        let listed = group.ordered(|uuid| {
            names
                .iter()
                .find(|(known, _)| *known == uuid)
                .map(|(_, name)| (*name).to_owned())
                .unwrap_or_default()
        });

        assert_eq!(
            listed.iter().map(|(_, name)| name.as_str()).collect::<Vec<_>>(),
            ["Wendy", "alice", "Zoe"]
        );
    }
}
