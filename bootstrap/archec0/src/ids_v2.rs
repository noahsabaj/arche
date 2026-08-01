use std::fmt;

const FINGERPRINT_VERSION: u32 = 1;
const SCHEMA_DOMAIN: &[u8] = b"ARCHE-SCHEMA-ID\0";
const SYSTEM_DOMAIN: &[u8] = b"ARCHE-SYSTEM-ID\0";
const SCHEDULE_DOMAIN: &[u8] = b"ARCHE-SCHEDULE-ID\0";
const QUERY_DOMAIN: &[u8] = b"ARCHE-QUERY-ID\0";
const ABI_DOMAIN: &[u8] = b"ARCHE-ABI-HASH\0";
const BODY_DOMAIN: &[u8] = b"ARCHE-BODY-HASH\0";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum SchemaKind {
    Component = 1,
    Resource = 2,
    Tag = 3,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum PrimitiveType {
    I32 = 1,
    F32 = 2,
    Bool = 3,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SchemaField<'a> {
    pub name: &'a str,
    pub primitive: PrimitiveType,
}

pub trait CanonicalId128 {
    fn as_bytes(&self) -> &[u8; 16];
}

macro_rules! fingerprint_type {
    ($name:ident) => {
        #[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name([u8; 16]);

        impl $name {
            pub const fn from_bytes(bytes: [u8; 16]) -> Self {
                Self(bytes)
            }

            pub const fn as_bytes(&self) -> &[u8; 16] {
                &self.0
            }

            pub const fn into_bytes(self) -> [u8; 16] {
                self.0
            }
        }

        impl CanonicalId128 for $name {
            fn as_bytes(&self) -> &[u8; 16] {
                self.as_bytes()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                for byte in self.0 {
                    write!(formatter, "{byte:02X}")?;
                }
                Ok(())
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(formatter, "{}({self})", stringify!($name))
            }
        }
    };
}

fingerprint_type!(SchemaId);
fingerprint_type!(DeclId);
fingerprint_type!(AbiHash);
fingerprint_type!(BodyHash);

impl SchemaId {
    pub fn derive(
        kind: SchemaKind,
        world: &str,
        local_name: &str,
        fields: &[SchemaField<'_>],
    ) -> Self {
        let mut hasher = CanonicalHasher::new(SCHEMA_DOMAIN);
        hasher.append_u8(kind as u8);
        hasher.append_string(world);
        hasher.append_string(local_name);
        hasher.append_u64(
            u64::try_from(fields.len()).expect("slice lengths fit the canonical u64 format"),
        );
        for field in fields {
            hasher.append_string(field.name);
            hasher.append_u8(field.primitive as u8);
        }
        Self(hasher.finalize())
    }
}

impl DeclId {
    pub fn system(world: &str, name: &str) -> Self {
        let mut hasher = CanonicalHasher::new(SYSTEM_DOMAIN);
        hasher.append_string(world);
        hasher.append_string(name);
        Self(hasher.finalize())
    }

    pub fn schedule(world: &str, name: &str) -> Self {
        let mut hasher = CanonicalHasher::new(SCHEDULE_DOMAIN);
        hasher.append_string(world);
        hasher.append_string(name);
        Self(hasher.finalize())
    }

    pub fn query(parent_system: Self, parameter_name: &str) -> Self {
        let mut hasher = CanonicalHasher::new(QUERY_DOMAIN);
        hasher.append_id(&parent_system);
        hasher.append_string(parameter_name);
        Self(hasher.finalize())
    }
}

struct CanonicalHasher {
    hasher: blake3::Hasher,
}

impl CanonicalHasher {
    fn new(domain: &[u8]) -> Self {
        debug_assert_eq!(domain.last(), Some(&0));
        let mut hasher = blake3::Hasher::new();
        hasher.update(domain);
        hasher.update(&FINGERPRINT_VERSION.to_le_bytes());
        Self { hasher }
    }

    fn append_u8(&mut self, value: u8) -> &mut Self {
        self.hasher.update(&[value]);
        self
    }

    fn append_u64(&mut self, value: u64) -> &mut Self {
        self.hasher.update(&value.to_le_bytes());
        self
    }

    fn append_id(&mut self, value: &impl CanonicalId128) -> &mut Self {
        self.hasher.update(value.as_bytes());
        self
    }

    fn append_string(&mut self, value: &str) -> &mut Self {
        let bytes = value.as_bytes();
        self.append_u64(
            u64::try_from(bytes.len()).expect("string lengths fit the canonical u64 format"),
        );
        self.hasher.update(bytes);
        self
    }

    fn finalize(self) -> [u8; 16] {
        let digest = self.hasher.finalize();
        let mut fingerprint = [0; 16];
        fingerprint.copy_from_slice(&digest.as_bytes()[..16]);
        fingerprint
    }
}

pub struct AbiHasher {
    canonical: CanonicalHasher,
}

impl AbiHasher {
    pub fn new() -> Self {
        Self {
            canonical: CanonicalHasher::new(ABI_DOMAIN),
        }
    }

    pub fn append_u8(&mut self, value: u8) -> &mut Self {
        self.canonical.append_u8(value);
        self
    }

    pub fn append_u64(&mut self, value: u64) -> &mut Self {
        self.canonical.append_u64(value);
        self
    }

    pub fn append_id(&mut self, value: &impl CanonicalId128) -> &mut Self {
        self.canonical.append_id(value);
        self
    }

    pub fn append_string(&mut self, value: &str) -> &mut Self {
        self.canonical.append_string(value);
        self
    }

    pub fn finalize(self) -> AbiHash {
        AbiHash(self.canonical.finalize())
    }
}

impl Default for AbiHasher {
    fn default() -> Self {
        Self::new()
    }
}

pub struct BodyHasher {
    canonical: CanonicalHasher,
}

impl BodyHasher {
    pub fn new() -> Self {
        Self {
            canonical: CanonicalHasher::new(BODY_DOMAIN),
        }
    }

    pub fn append_u8(&mut self, value: u8) -> &mut Self {
        self.canonical.append_u8(value);
        self
    }

    pub fn append_u64(&mut self, value: u64) -> &mut Self {
        self.canonical.append_u64(value);
        self
    }

    pub fn append_id(&mut self, value: &impl CanonicalId128) -> &mut Self {
        self.canonical.append_id(value);
        self
    }

    pub fn append_string(&mut self, value: &str) -> &mut Self {
        self.canonical.append_string(value);
        self
    }

    pub fn finalize(self) -> BodyHash {
        BodyHash(self.canonical.finalize())
    }
}

impl Default for BodyHasher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const POSITION_SCHEMA_GOLDEN: &str = "E6E38FA83F96A32AA6CA26FCD8E29FED";
    const MOVE_SYSTEM_GOLDEN: &str = "30B49813C21A4FE2AC3AB5EC91762525";
    const MAIN_SCHEDULE_GOLDEN: &str = "A84CD595F7D399E6B08123EF5DAA90F5";
    const MOVERS_QUERY_GOLDEN: &str = "CB0E807161BB02CAB685F0AC9C9BF4DC";
    const ABI_GOLDEN: &str = "9271D60459527794206B07D1568A7D94";
    const BODY_GOLDEN: &str = "45054B4897A991ABA55F35AEBF36CAF1";

    #[test]
    fn schema_id_has_a_canonical_golden_value() {
        let id = SchemaId::derive(
            SchemaKind::Component,
            "Demo",
            "Position",
            &[
                SchemaField {
                    name: "x",
                    primitive: PrimitiveType::F32,
                },
                SchemaField {
                    name: "y",
                    primitive: PrimitiveType::F32,
                },
            ],
        );

        assert_eq!(id.to_string(), POSITION_SCHEMA_GOLDEN);
    }

    #[test]
    fn declaration_ids_have_canonical_golden_values() {
        let system = DeclId::system("Demo", "Move");
        let schedule = DeclId::schedule("Demo", "Main");
        let query = DeclId::query(system, "movers");

        assert_eq!(system.to_string(), MOVE_SYSTEM_GOLDEN);
        assert_eq!(schedule.to_string(), MAIN_SCHEDULE_GOLDEN);
        assert_eq!(query.to_string(), MOVERS_QUERY_GOLDEN);
    }

    #[test]
    fn abi_and_body_hashes_are_domain_separated_goldens() {
        let schema = SchemaId::derive(
            SchemaKind::Resource,
            "Demo",
            "Time",
            &[SchemaField {
                name: "delta",
                primitive: PrimitiveType::F32,
            }],
        );
        let mut abi = AbiHasher::new();
        abi.append_u8(7)
            .append_u64(0x0102_0304_0506_0708)
            .append_id(&schema)
            .append_string("Move");
        let mut body = BodyHasher::new();
        body.append_u8(7)
            .append_u64(0x0102_0304_0506_0708)
            .append_id(&schema)
            .append_string("Move");

        let abi = abi.finalize();
        let body = body.finalize();
        assert_eq!(abi.to_string(), ABI_GOLDEN);
        assert_eq!(body.to_string(), BODY_GOLDEN);
        assert_ne!(abi.as_bytes(), body.as_bytes());
    }

    #[test]
    fn display_is_exact_uppercase_wire_order() {
        let id = SchemaId::from_bytes([
            0x00, 0x01, 0x0a, 0x0f, 0x10, 0x11, 0x7f, 0x80, 0xa0, 0xaf, 0xf0, 0xf1, 0xfc, 0xfd,
            0xfe, 0xff,
        ]);

        assert_eq!(id.to_string(), "00010A0F10117F80A0AFF0F1FCFDFEFF");
    }

    #[test]
    fn field_order_and_schema_kind_are_part_of_the_schema_id() {
        let fields = [
            SchemaField {
                name: "x",
                primitive: PrimitiveType::F32,
            },
            SchemaField {
                name: "y",
                primitive: PrimitiveType::F32,
            },
        ];
        let reversed = [fields[1], fields[0]];

        assert_ne!(
            SchemaId::derive(SchemaKind::Component, "Demo", "Position", &fields),
            SchemaId::derive(SchemaKind::Component, "Demo", "Position", &reversed)
        );
        assert_ne!(
            SchemaId::derive(SchemaKind::Component, "Demo", "Position", &fields),
            SchemaId::derive(SchemaKind::Tag, "Demo", "Position", &fields)
        );
    }

    #[test]
    fn length_prefixes_prevent_string_boundary_ambiguity() {
        assert_ne!(DeclId::system("ab", "c"), DeclId::system("a", "bc"));
    }

    #[test]
    fn unicode_strings_use_utf8_byte_lengths_in_the_canonical_preimage() {
        assert_eq!(
            DeclId::system("Δ", "名").to_string(),
            "F47D45318BA812D92B71FC7CA4C51302"
        );
    }

    #[test]
    fn schema_discriminants_are_the_canonical_wire_codes() {
        assert_eq!(
            [
                SchemaKind::Component as u8,
                SchemaKind::Resource as u8,
                SchemaKind::Tag as u8,
            ],
            [1, 2, 3]
        );
        assert_eq!(
            [
                PrimitiveType::I32 as u8,
                PrimitiveType::F32 as u8,
                PrimitiveType::Bool as u8,
            ],
            [1, 2, 3]
        );
    }
}
