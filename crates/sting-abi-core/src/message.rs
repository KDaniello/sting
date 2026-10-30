use crate::TypeRef;

/// Points at one message body's shape, as referenced from
/// [`ContractSchema::incoming_internal`](crate::ContractSchema::incoming_internal)/
/// [`incoming_external`](crate::ContractSchema::incoming_external)/
/// [`outgoing`](crate::ContractSchema::outgoing). The body's own name/opcode
/// live on the [`NamedType`](crate::NamedType) this points at — kept there,
/// not duplicated here, since the same message type can in principle be
/// referenced from more than one of the three lists (e.g. a message a
/// contract both accepts and forwards unchanged).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MessageRef {
    /// Index into [`ContractSchema::types`](crate::ContractSchema::types)
    /// for this message's body type — always a
    /// [`TypeShape::Struct`](crate::TypeShape::Struct) in practice, but not
    /// narrowed to that at the type level, mirroring how Tolk's own
    /// `MessageRef` just points at a `unique_types` index without
    /// restricting its shape.
    pub ty: TypeRef,
}
