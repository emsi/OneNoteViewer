//! File system abstraction used by the OneNote parser.

use bytes::Bytes;

use crate::fs::file_source::BytesSource;
use std::io::{Error, Read};
use std::sync::Arc;
use typed_path::{TypedPath, TypedPathBuf};

pub mod file_source;
#[cfg(feature = "native-fs")]
pub mod native_fs;

/// Abstraction over file system operations.
///
/// This trait provides an interface for file system operations used by the OneNote parser.
/// It enables dependency injection for testing and alternative file system implementations.
///
/// All implementations must be thread-safe (`Send + Sync`) as the parser may be used
/// across threads.
///
/// Implementations must also be `Copy` so callers can pass the filesystem handle
/// by value.
///
/// # Security contract for path-taking methods
///
/// See the crate-level [Path handling](crate#path-handling) section for
/// how callers construct the `TypedPath`s that reach this trait. This
/// section covers the *outbound* half — what an implementation must
/// guarantee when translating those paths to the underlying storage.
///
/// Paths handed to this trait by the parser come from two sources:
///
/// 1. Paths supplied directly by the caller via [`crate::Parser::parse_notebook`]
///    or [`crate::Parser::parse_section`]. These are trusted.
/// 2. Paths derived from `.onetoc2` section entries. These are
///    **attacker-controlled** — a malicious notebook can put anything
///    in there. The parser strips path-traversal structure (absolute
///    paths, `..`, drive prefixes) and runs character-level
///    sanitisation before calling into the implementation, but it
///    **cannot** defend against host-specific filesystem quirks because
///    it doesn't know which host the implementation runs on.
///
/// Implementations are therefore responsible for ensuring that opening,
/// stat-ing, listing, or existence-checking a path cannot have
/// unintended side effects on the host. The known failure mode is
/// **Windows reserved device names** (`CON`, `PRN`, `AUX`, `NUL`,
/// `COM0`–`COM9`, `LPT0`–`LPT9`): on a Windows host, standard Win32
/// file APIs interpret a path whose basename matches one of these
/// (with or without an extension) as a handle to the corresponding
/// device — `std::fs::File::open("COM1")` opens the COM1 serial port,
/// not a file. Implementations that may run on Windows must either:
///
/// - reject such paths upfront, or
/// - prepend `\\?\` (Windows verbatim namespace) to an absolute,
///   backslash-form path before invoking the underlying open. This
///   applies to native Windows builds and to WASM builds whose
///   JS shim ultimately invokes `fs.openSync` on Windows Node.js.
///
/// The bundled [`NativeFs`] impl handles this. External impls
/// (WASM / JS shims / custom storage) must do the equivalent
/// themselves.
///
/// Write methods ([`Self::write_file`], [`Self::stream_to_file`],
/// [`Self::make_dir`]) are out of scope for this contract: write paths
/// originate with the user, not with parsed input (we don't write files
/// during parsing), and users of this library (e.g. one2html) are
/// expected to sanitise output filenames before handing them to this
/// trait.
pub trait FileSystem: Send + Sync + Copy {
    /// Checks if the given path points to a directory.
    ///
    /// Mirrors the semantics of [`std::path::Path::is_dir`]: a missing path is
    /// not an error. Use [`FileSystem::exists`] if the existence check itself
    /// matters.
    ///
    /// # Arguments
    /// * `path` - The path to check
    ///
    /// # Returns
    /// * `Ok(true)` if the path exists and is a directory
    /// * `Ok(false)` if the path does not exist, or exists but is not a directory
    /// * `Err` only on I/O errors that aren't "not found" (e.g. permission denied)
    ///
    /// # Usage
    /// Used by the parser to distinguish between section files (.one) and section groups
    /// (directories containing .onetoc2 files).
    fn is_directory(&self, path: TypedPath) -> Result<bool, Error>;

    /// Lists all entries in a directory.
    ///
    /// # Arguments
    /// * `path` - The directory path to read
    ///
    /// # Returns
    /// A vector of paths for all entries in the directory, or an error if the
    /// directory cannot be read.
    ///
    /// # Usage
    /// Used to enumerate section files and subdirectories when parsing section groups.
    fn read_dir(&self, path: TypedPath) -> Result<Vec<TypedPathBuf>, Error>;

    /// Reads the entire contents of a file into memory.
    ///
    /// # Arguments
    /// * `path` - The file path to read
    ///
    /// # Returns
    /// The complete file contents as a byte vector, or an error if the file
    /// cannot be read.
    ///
    /// # Usage
    /// Used to load OneNote files (.one, .onetoc2) for parsing. Files are read
    /// entirely into memory as the parser needs random access to the data.
    fn read_file(&self, path: TypedPath) -> Result<Vec<u8>, Error>;

    /// Writes data to a file, replacing any existing content.
    ///
    /// # Arguments
    /// * `path` - The file path to write to
    /// * `data` - The data to write
    ///
    /// # Returns
    /// Ok(()) on success, or an error if the file cannot be written.
    ///
    /// # Usage
    /// May be used for extracting embedded content or creating output files.
    fn write_file(&self, path: TypedPath, data: &[u8]) -> Result<(), Error>;

    /// Stream the contents of `reader` to a file, replacing any existing
    /// content.
    ///
    /// Intended for writing large attachment payloads out without
    /// materialising them in memory. Implementations should write in
    /// fixed-size chunks (`std::io::copy` on native, chunked
    /// `appendFileSync`-style writes on WASM) rather than buffering the
    /// whole stream — for that you can call [`FileSystem::write_file`]
    /// directly.
    ///
    /// # Errors
    ///
    /// On failure mid-stream the destination file may be left in a
    /// partially-written state.
    fn stream_to_file(&self, path: TypedPath, reader: &mut dyn Read) -> Result<(), Error>;

    /// Creates a directory, including any missing parent directories.
    ///
    /// # Arguments
    /// * `path` - The directory path to create
    ///
    /// # Returns
    /// `Ok(())` if the directory was created or already exists as a directory,
    /// or an error if the directory cannot be created.
    ///
    /// # Note
    /// This method is idempotent when `path` already exists as a directory.
    /// If `path` exists but is not a directory (e.g. a regular file or a
    /// symlink to one), implementations must return an error rather than
    /// silently succeeding.
    fn make_dir(&self, path: TypedPath) -> Result<(), Error>;

    /// Resolve `path` to a canonical, symlink-followed form.
    ///
    /// Used to verify that resolved section-entry paths do not escape the
    /// notebook base directory through symlinks.
    fn canonicalize(&self, path: TypedPath) -> Result<TypedPathBuf, Error>;

    /// Checks if a path exists in the file system.
    ///
    /// # Arguments
    /// * `path` - The path to check
    ///
    /// # Returns
    /// * `Ok(true)` if the path exists (file or directory)
    /// * `Ok(false)` if the path does not exist
    /// * `Err` if the existence check fails due to permissions or other I/O errors
    ///
    /// # Usage
    /// Used to filter out non-existent section entries and verify paths before
    /// attempting to parse them.
    fn exists(&self, path: TypedPath) -> Result<bool, Error>;

    /// Opens a file as a [`FileSource`] for parsing.
    ///
    /// The default implementation reads the entire file via
    /// [`FileSystem::read_file`] and wraps the resulting buffer in a
    /// [`BytesSource`] — eager, simple, and suitable for any backend that
    /// can hand back a `Vec<u8>`. Implementations that can serve bytes
    /// without materialising the whole file in process memory (positional
    /// disk reads, WASM-side `Blob`-chunked reads) should override this.
    ///
    /// Callers must not mutate the underlying file while the returned
    /// [`FileSource`], or any attachment refcount-shared off it, is alive.
    fn open_file(&self, path: TypedPath) -> Result<Arc<dyn FileSource>, Error> {
        let bytes = Bytes::from(self.read_file(path)?);
        Ok(Arc::new(BytesSource::new(bytes)))
    }

    /// Checks if the current operating system is Windows.
    ///
    /// # Returns
    /// * `true` if the code is being compiled and run on a Windows operating system.
    /// * `false` otherwise.
    fn is_windows(&self) -> bool {
        cfg!(windows)
    }
}

/// A random-access byte source backing a parse.
///
/// The parser reads notebook data through this trait. Reads take an
/// absolute `offset` and may happen in any order; the trait holds no
/// position of its own.
///
/// If you can hand the parser an in-memory `Bytes`, you almost certainly
/// don't need to implement this trait — use [`BytesSource`] (or the
/// default [`FileSystem::open_file`] impl). Implement `FileSource`
/// directly when you want to serve bytes without materialising the whole
/// file in memory (e.g. a WASM `Blob` you read chunks from on demand).
///
/// Implementations are `Send + Sync` and the returned [`Bytes`] are too.
pub trait FileSource: Send + Sync {
    /// Total length of the underlying source in bytes.
    fn byte_length(&self) -> u64;

    /// Read `len` bytes starting at absolute `offset`.
    ///
    /// For in-memory backings, return a refcount-shared slice
    /// (`Bytes::slice`); for backings that fetch on demand, allocate.
    fn read_at(&self, offset: u64, len: usize) -> Result<Bytes, Error>;

    /// Return the entire source as a refcount-shared buffer when it is
    /// fully held in memory.
    ///
    /// The parser uses this as a fast path for hot per-byte indexing.
    /// Return `None` if the bytes aren't all resident; the parser will
    /// fall back to [`read_at`](FileSource::read_at).
    fn as_bytes(&self) -> Option<Bytes> {
        None
    }
}
