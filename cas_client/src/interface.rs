use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use cas_types::{FileRange, QueryReconstructionResponse};
use mdb_shard::shard_file_reconstructor::FileReconstructor;
use merklehash::MerkleHash;
use utils::progress::ProgressUpdater;

use crate::error::Result;
use crate::CasClientError;

/// A Client to the CAS (Content Addressed Storage) service to allow storage and
/// management of XORBs (Xet Object Remote Block). A XORB represents a collection
/// of arbitrary bytes. These bytes are hashed according to a Xet Merkle Hash
/// producing a Merkle Tree. XORBs in the CAS are identified by a combination of
/// a prefix namespacing the XORB and the hash at the root of the Merkle Tree.
#[async_trait]
pub trait UploadClient {
    /// Insert the provided data into the CAS as a XORB indicated by the prefix and hash.
    /// The hash will be verified on the SERVER-side according to the chunk boundaries.
    /// Chunk Boundaries must be complete; i.e. the last entry in chunk boundary
    /// must be the length of data. For instance, if data="helloworld" with 2 chunks
    /// ["hello" "world"], chunk_boundaries should be [5, 10].
    /// Empty data and empty chunk boundaries are not accepted.
    ///
    /// Note that put may background in some implementations and a flush()
    /// will be needed.
    async fn put(
        &self,
        prefix: &str,
        hash: &MerkleHash,
        data: Vec<u8>,
        chunk_and_boundaries: Vec<(MerkleHash, u32)>,
    ) -> Result<usize>;

    /// Check if a XORB already exists.
    async fn exists(&self, prefix: &str, hash: &MerkleHash) -> Result<bool>;
}

/// A Client to the CAS (Content Addressed Storage) service to allow reconstructing a
/// pointer file based on FileID (MerkleHash).
///
/// To simplify this crate, it is intentional that the client does not create its own http_client or
/// spawn its own threads. Instead, it is expected to be given the parallelism harness/threadpool/queue
/// on which it is expected to run. This allows the caller to better optimize overall system utilization
/// by controlling the number of concurrent requests.
#[async_trait]
pub trait ReconstructionClient {
    /// Get an entire file by file hash with an optional bytes range.
    ///
    /// The http_client passed in is a non-authenticated client. This is used to directly communicate
    /// with the backing store (S3) to retrieve xorbs.
    async fn get_file(
        &self,
        hash: &MerkleHash,
        byte_range: Option<FileRange>,
        writer: &WriteProvider,
        progress_updater: Option<Arc<dyn ProgressUpdater>>,
    ) -> Result<u64>;

    async fn batch_get_file(&self, files: HashMap<MerkleHash, &WriteProvider>) -> Result<u64> {
        let mut n_bytes = 0;
        // Provide the basic naive implementation as a default.
        for (h, w) in files {
            n_bytes += self.get_file(&h, None, w, None).await?;
        }
        Ok(n_bytes)
    }
}

/// Enum of different output formats to write reconstructed files to.
#[derive(Debug, Clone)]
pub enum WriteProvider {
    File(FileWriteProvider),
    #[cfg(test)]
    Buffer(buffer::BufferProvider),
}

impl WriteProvider {
    /// Create a new writer to start writing at the indicated start location.
    pub(crate) fn get_writer_at(&self, start: u64) -> Result<Box<dyn Write + Send>> {
        match self {
            WriteProvider::File(fp) => fp.get_writer_at(start).map(|x| Box::new(x) as Box<dyn Write + Send>),
            #[cfg(test)]
            WriteProvider::Buffer(bp) => bp.get_writer_at(start).map(|x| Box::new(x) as Box<dyn Write + Send>),
        }
    }
}

#[derive(Debug, Clone)]
pub struct FileWriteProvider {
    filename: PathBuf,
}

impl FileWriteProvider {
    pub fn new(filename: PathBuf) -> Self {
        Self { filename }
    }

    fn get_writer_at(&self, start: u64) -> Result<File> {
        let mut file = OpenOptions::new()
            .write(true)
            .truncate(false)
            .create(true)
            .open(&self.filename)?;
        file.seek(SeekFrom::Start(start))?;
        Ok(file)
    }
}

/// A Client to the CAS (Content Addressed Storage) service that is able to obtain
/// the reconstruction info of a file by FileID (MerkleHash).
/// This trait is meant for internal (caching): external users to this crate don't
/// access these trait functions.
#[async_trait]
pub(crate) trait Reconstructable {
    async fn get_reconstruction(
        &self,
        hash: &MerkleHash,
        byte_range: Option<FileRange>,
    ) -> Result<QueryReconstructionResponse>;
}

/// Probes for shards that provide dedup information for a chunk, and, if
/// any are found, writes them to disk and returns the path.
#[async_trait]
pub trait ShardDedupProber {
    async fn query_for_global_dedup_shard(
        &self,
        prefix: &str,
        chunk_hash: &MerkleHash,
        salt: &[u8; 32],
    ) -> Result<Option<PathBuf>>;
}

#[async_trait]
pub trait RegistrationClient {
    async fn upload_shard(
        &self,
        prefix: &str,
        hash: &MerkleHash,
        force_sync: bool,
        shard_data: &[u8],
        salt: &[u8; 32],
    ) -> Result<bool>;
}

/// A Client to the Shard service. The shard service
/// provides for
/// 1. upload shard to the shard service
/// 2. querying of file->reconstruction information
/// 3. querying of chunk->shard information
pub trait ShardClientInterface:
    RegistrationClient + FileReconstructor<CasClientError> + ShardDedupProber + Send + Sync
{
}

pub trait Client: UploadClient + ReconstructionClient + ShardClientInterface {}

#[cfg(test)]
pub mod buffer {
    use std::io::Cursor;
    use std::sync::Mutex;

    use super::*;

    #[derive(Debug, Default, Clone)]
    pub struct BufferProvider {
        pub buf: ThreadSafeBuffer,
    }

    impl BufferProvider {
        pub fn get_writer_at(&self, _start: u64) -> Result<ThreadSafeBuffer> {
            Ok(self.buf.clone()) // TODO: fix tests once we start writing in parallel
        }
    }

    #[derive(Debug, Default, Clone)]
    /// Thread-safe in-memory buffer that implements [Write](Write) trait and allows
    /// access to inner buffer
    pub struct ThreadSafeBuffer {
        inner: Arc<Mutex<Cursor<Vec<u8>>>>,
    }

    impl ThreadSafeBuffer {
        pub fn value(&self) -> Vec<u8> {
            self.inner.lock().unwrap().get_ref().clone()
        }
    }

    impl Write for ThreadSafeBuffer {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.inner.lock().map_err(|e| std::io::Error::other(format!("{e}")))?.write(buf)
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
}
