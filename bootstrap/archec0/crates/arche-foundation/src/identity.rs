//! Domain-separated 128-bit identity scaffolding for M27.
//!
//! These types do not replace or reinterpret the M26 schema and declaration
//! identities. Later M27 gates define the canonical preimage for each item and
//! then construct the corresponding type through this boundary.

use std::fmt;

/// M27 identities deliberately use fingerprint version 2. M26 identities keep
/// their historical version-1 prefixes and golden vectors in `archec0`.
pub const IDENTITY_FINGERPRINT_VERSION: u32 = 2;

pub const PACKAGE_PREFIX: &[u8] = b"ARCHE-PACKAGE-ID\0\x02\x00\x00\x00";
pub const DEFINITION_PREFIX: &[u8] = b"ARCHE-DEF-ID\0\x02\x00\x00\x00";
pub const TYPE_PREFIX: &[u8] = b"ARCHE-TYPE-ID\0\x02\x00\x00\x00";
pub const INSTANCE_PREFIX: &[u8] = b"ARCHE-INSTANCE-ID\0\x02\x00\x00\x00";
pub const INTERFACE_PREFIX: &[u8] = b"ARCHE-INTERFACE-HASH\0\x02\x00\x00\x00";
pub const LAYOUT_PREFIX: &[u8] = b"ARCHE-LAYOUT-HASH\0\x02\x00\x00\x00";
pub const ABI_PREFIX: &[u8] = b"ARCHE-ABI-HASH\0\x02\x00\x00\x00";
pub const BODY_PREFIX: &[u8] = b"ARCHE-BODY-HASH\0\x02\x00\x00\x00";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Domain {
    Package,
    Definition,
    Type,
    Instance,
    Interface,
    Layout,
    Abi,
    Body,
}

impl Domain {
    pub const ALL: [Self; 8] = [
        Self::Package,
        Self::Definition,
        Self::Type,
        Self::Instance,
        Self::Interface,
        Self::Layout,
        Self::Abi,
        Self::Body,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Package => "package ID",
            Self::Definition => "definition ID",
            Self::Type => "type ID",
            Self::Instance => "instance ID",
            Self::Interface => "interface hash",
            Self::Layout => "layout hash",
            Self::Abi => "ABI hash",
            Self::Body => "Core body hash",
        }
    }

    pub const fn bytes(self) -> &'static [u8] {
        match self {
            Self::Package => b"ARCHE-PACKAGE-ID\0",
            Self::Definition => b"ARCHE-DEF-ID\0",
            Self::Type => b"ARCHE-TYPE-ID\0",
            Self::Instance => b"ARCHE-INSTANCE-ID\0",
            Self::Interface => b"ARCHE-INTERFACE-HASH\0",
            Self::Layout => b"ARCHE-LAYOUT-HASH\0",
            Self::Abi => b"ARCHE-ABI-HASH\0",
            Self::Body => b"ARCHE-BODY-HASH\0",
        }
    }

    pub const fn prefix(self) -> &'static [u8] {
        match self {
            Self::Package => PACKAGE_PREFIX,
            Self::Definition => DEFINITION_PREFIX,
            Self::Type => TYPE_PREFIX,
            Self::Instance => INSTANCE_PREFIX,
            Self::Interface => INTERFACE_PREFIX,
            Self::Layout => LAYOUT_PREFIX,
            Self::Abi => ABI_PREFIX,
            Self::Body => BODY_PREFIX,
        }
    }
}

pub trait Identity128 {
    fn as_bytes(&self) -> &[u8; 16];
}

macro_rules! identity_type {
    ($name:ident, $domain:ident) => {
        #[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name([u8; 16]);

        impl $name {
            pub const DOMAIN: Domain = Domain::$domain;

            pub const fn from_bytes(bytes: [u8; 16]) -> Self {
                Self(bytes)
            }

            pub fn from_canonical_preimage(preimage: &[u8]) -> Self {
                Self(derive(Self::DOMAIN, preimage))
            }

            pub const fn as_bytes(&self) -> &[u8; 16] {
                &self.0
            }

            pub const fn into_bytes(self) -> [u8; 16] {
                self.0
            }
        }

        impl Identity128 for $name {
            fn as_bytes(&self) -> &[u8; 16] {
                self.as_bytes()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                write_upper_hex(formatter, &self.0)
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(formatter, "{}({self})", stringify!($name))
            }
        }
    };
}

identity_type!(PackageId, Package);
identity_type!(DefinitionId, Definition);
identity_type!(TypeId, Type);
identity_type!(InstanceId, Instance);
identity_type!(InterfaceHash, Interface);
identity_type!(LayoutHash, Layout);
identity_type!(AbiHash, Abi);
identity_type!(CoreBodyHash, Body);

pub type DefId = DefinitionId;

fn derive(domain: Domain, preimage: &[u8]) -> [u8; 16] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(domain.prefix());
    hasher.update(preimage);
    let digest = hasher.finalize();
    let mut id = [0_u8; 16];
    id.copy_from_slice(&digest.as_bytes()[..16]);
    id
}

fn write_upper_hex(formatter: &mut fmt::Formatter<'_>, bytes: &[u8]) -> fmt::Result {
    for byte in bytes {
        write!(formatter, "{byte:02X}")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn domains_are_exact_unique_and_nul_terminated() {
        assert_eq!(Domain::Package.bytes(), b"ARCHE-PACKAGE-ID\0");
        assert_eq!(Domain::Definition.bytes(), b"ARCHE-DEF-ID\0");
        assert_eq!(Domain::Type.bytes(), b"ARCHE-TYPE-ID\0");
        assert_eq!(Domain::Instance.bytes(), b"ARCHE-INSTANCE-ID\0");
        assert_eq!(Domain::Interface.bytes(), b"ARCHE-INTERFACE-HASH\0");
        assert_eq!(Domain::Layout.bytes(), b"ARCHE-LAYOUT-HASH\0");
        assert_eq!(Domain::Abi.bytes(), b"ARCHE-ABI-HASH\0");
        assert_eq!(Domain::Body.bytes(), b"ARCHE-BODY-HASH\0");

        let domains = Domain::ALL
            .into_iter()
            .map(Domain::bytes)
            .collect::<BTreeSet<_>>();
        assert_eq!(domains.len(), Domain::ALL.len());
        assert!(domains.iter().all(|domain| domain.ends_with(&[0])));
    }

    #[test]
    fn m27_preimage_prefixes_are_byte_exact_version_two() {
        assert_eq!(
            Domain::Package.prefix(),
            b"ARCHE-PACKAGE-ID\0\x02\x00\x00\x00"
        );
        assert_eq!(
            Domain::Definition.prefix(),
            b"ARCHE-DEF-ID\0\x02\x00\x00\x00"
        );
        assert_eq!(Domain::Type.prefix(), b"ARCHE-TYPE-ID\0\x02\x00\x00\x00");
        assert_eq!(
            Domain::Instance.prefix(),
            b"ARCHE-INSTANCE-ID\0\x02\x00\x00\x00"
        );
        assert_eq!(
            Domain::Interface.prefix(),
            b"ARCHE-INTERFACE-HASH\0\x02\x00\x00\x00"
        );
        assert_eq!(
            Domain::Layout.prefix(),
            b"ARCHE-LAYOUT-HASH\0\x02\x00\x00\x00"
        );
        assert_eq!(Domain::Abi.prefix(), b"ARCHE-ABI-HASH\0\x02\x00\x00\x00");
        assert_eq!(Domain::Body.prefix(), b"ARCHE-BODY-HASH\0\x02\x00\x00\x00");
        for domain in Domain::ALL {
            let prefix = domain.prefix();
            assert_eq!(
                &prefix[prefix.len() - 4..],
                &IDENTITY_FINGERPRINT_VERSION.to_le_bytes()
            );
        }
    }

    #[test]
    fn typed_id_domains_are_separated() {
        let preimage = b"arche/example\0src/main.arc\0World";
        let values = [
            PackageId::from_canonical_preimage(preimage).into_bytes(),
            DefinitionId::from_canonical_preimage(preimage).into_bytes(),
            TypeId::from_canonical_preimage(preimage).into_bytes(),
            InstanceId::from_canonical_preimage(preimage).into_bytes(),
            InterfaceHash::from_canonical_preimage(preimage).into_bytes(),
            LayoutHash::from_canonical_preimage(preimage).into_bytes(),
            AbiHash::from_canonical_preimage(preimage).into_bytes(),
            CoreBodyHash::from_canonical_preimage(preimage).into_bytes(),
        ];
        assert_eq!(values.into_iter().collect::<BTreeSet<_>>().len(), 8);
    }

    #[test]
    fn typed_ids_have_canonical_golden_values() {
        let preimage = b"arche/example\0src/main.arc\0World";
        let actual = [
            PackageId::from_canonical_preimage(preimage).to_string(),
            DefinitionId::from_canonical_preimage(preimage).to_string(),
            TypeId::from_canonical_preimage(preimage).to_string(),
            InstanceId::from_canonical_preimage(preimage).to_string(),
            InterfaceHash::from_canonical_preimage(preimage).to_string(),
            LayoutHash::from_canonical_preimage(preimage).to_string(),
            AbiHash::from_canonical_preimage(preimage).to_string(),
            CoreBodyHash::from_canonical_preimage(preimage).to_string(),
        ];
        assert_eq!(
            actual,
            [
                "106B8743328E2D24B527801CC7CE7027",
                "6D7F9033224BB4C2401743D9714EB1AC",
                "58969672741C381126D0EA189DA085D0",
                "9F46B71B54B3E4E9C13AE21BF486DA5E",
                "40B22C4686E70E8D9EC9086C78CAF77D",
                "009F71385F8EFC184BC902B18E473767",
                "A3F6321567C17BCF709D720007562253",
                "7CA2FB4E2ECCB4C607CBC23D8922CBFD",
            ]
        );
    }
}
