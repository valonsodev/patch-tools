/// Dex method access flags matching dexlib2's `AccessFlags` values.
#[derive(Clone, Copy)]
#[repr(i32)]
pub(crate) enum DexAccessFlag {
    Public = 0x0001,
    Private = 0x0002,
    Protected = 0x0004,
    Static = 0x0008,
    Final = 0x0010,
    Synchronized = 0x0020,
    Bridge = 0x0040,
    Varargs = 0x0080,
    Native = 0x0100,
    Interface = 0x0200,
    Abstract = 0x0400,
    Strictfp = 0x0800,
    Synthetic = 0x1000,
    Annotation = 0x2000,
    Enum = 0x4000,
    Constructor = 0x10000,
    DeclaredSynchronized = 0x20000,
}

impl DexAccessFlag {
    pub(crate) const ALL: &'static [Self] = &[
        Self::Public,
        Self::Private,
        Self::Protected,
        Self::Static,
        Self::Final,
        Self::Synchronized,
        Self::Bridge,
        Self::Varargs,
        Self::Native,
        Self::Interface,
        Self::Abstract,
        Self::Strictfp,
        Self::Synthetic,
        Self::Annotation,
        Self::Enum,
        Self::Constructor,
        Self::DeclaredSynchronized,
    ];

    pub(crate) const MAP_SIMILARITY_RELEVANT: &'static [Self] = &[
        Self::Public,
        Self::Private,
        Self::Protected,
        Self::Static,
        Self::Final,
        Self::Abstract,
        Self::Constructor,
    ];

    pub(crate) fn mask(self) -> i32 {
        self as i32
    }

    pub(crate) fn is_set(self, flags: i32) -> bool {
        flags & self.mask() != 0
    }

    pub(crate) fn dexlib_name(self) -> &'static str {
        match self {
            Self::Public => "PUBLIC",
            Self::Private => "PRIVATE",
            Self::Protected => "PROTECTED",
            Self::Static => "STATIC",
            Self::Final => "FINAL",
            Self::Synchronized => "SYNCHRONIZED",
            Self::Bridge => "BRIDGE",
            Self::Varargs => "VARARGS",
            Self::Native => "NATIVE",
            Self::Interface => "INTERFACE",
            Self::Abstract => "ABSTRACT",
            Self::Strictfp => "STRICTFP",
            Self::Synthetic => "SYNTHETIC",
            Self::Annotation => "ANNOTATION",
            Self::Enum => "ENUM",
            Self::Constructor => "CONSTRUCTOR",
            Self::DeclaredSynchronized => "DECLARED_SYNCHRONIZED",
        }
    }

    pub(crate) fn mask_for(flags: &[Self]) -> i32 {
        flags.iter().fold(0, |mask, flag| mask | flag.mask())
    }
}
