use std::collections::HashMap;
use std::convert::TryFrom;
use std::path::PathBuf;
use std::pin::Pin;

use async_trait::async_trait;
use futures::Future;

use error::*;
use generic::{Id, PathSegment};
use transact::fs;
use transact::lock::{Mutable, TxnLock};
use transact::TxnId;

use crate::chain::ChainBlock;
use crate::state::StateType;

use super::{dir_contents, file_name, fs_path, Cache, DirContents, File};

#[derive(Clone)]
pub enum FileEntry {
    Chain(File<ChainBlock>),
}

impl FileEntry {
    fn new(cache: Cache, path: PathBuf, class: StateType) -> TCResult<Self> {
        match class {
            StateType::Chain(_) => Ok(Self::Chain(File::new(cache, path))),
            other => Err(TCError::bad_request("cannot create file for", other)),
        }
    }
}

impl TryFrom<FileEntry> for File<ChainBlock> {
    type Error = TCError;

    fn try_from(file: FileEntry) -> TCResult<File<ChainBlock>> {
        match file {
            FileEntry::Chain(file) => Ok(file),
        }
    }
}

#[derive(Clone)]
pub enum DirEntry {
    Dir(Dir),
    File(FileEntry),
}

#[derive(Clone)]
pub struct Dir {
    cache: Cache,
    path: PathBuf,
    entries: TxnLock<Mutable<HashMap<PathSegment, DirEntry>>>,
}

impl Dir {
    fn new(cache: Cache, path: PathBuf, entries: HashMap<PathSegment, DirEntry>) -> Self {
        let entries = TxnLock::new(format!("directory at {:?}", path), entries.into());
        Self {
            cache,
            path,
            entries,
        }
    }

    fn load(
        cache: Cache,
        path: PathBuf,
        contents: DirContents,
    ) -> Pin<Box<dyn Future<Output = TCResult<Self>>>> {
        Box::pin(async move {
            if contents.iter().all(|(_, meta)| meta.is_dir()) {
                let mut entries = HashMap::new();

                for (handle, _) in contents.into_iter() {
                    let name = file_name(&handle)?;
                    let path = fs_path(&path, &name);
                    let contents = dir_contents(&path).await?;
                    if contents.iter().all(|(_, meta)| meta.is_file()) {
                        // TODO: support other file types
                        let file = File::load(cache.clone(), path, contents).await?;
                        entries.insert(name, DirEntry::File(FileEntry::Chain(file)));
                    } else if contents.iter().all(|(_, meta)| meta.is_dir()) {
                        let dir = Dir::load(cache.clone(), path, contents).await?;
                        entries.insert(name, DirEntry::Dir(dir));
                    } else {
                        return Err(TCError::internal(format!(
                            "directory at {:?} contains both blocks and subdirectories",
                            path
                        )));
                    }
                }

                Ok(Self::new(cache, path, entries))
            } else {
                Err(TCError::internal(format!(
                    "directory at {:?} contains both blocks and subdirectories",
                    path
                )))
            }
        })
    }
}

#[async_trait]
impl fs::Dir for Dir {
    type Class = StateType;
    type File = FileEntry;

    async fn create_dir(&self, txn_id: TxnId, name: PathSegment) -> TCResult<Self> {
        let path = fs_path(&self.path, &name);
        let dir = Dir::new(self.cache.clone(), path, HashMap::new());

        let mut entries = self.entries.write(txn_id).await?;
        entries.insert(name, DirEntry::Dir(dir.clone()));

        Ok(dir)
    }

    async fn create_file(&self, txn_id: TxnId, name: Id, class: StateType) -> TCResult<Self::File> {
        let path = fs_path(&self.path, &name);
        let file = FileEntry::new(self.cache.clone(), path, class)?;

        let mut entries = self.entries.write(txn_id).await?;
        entries.insert(name, DirEntry::File(file.clone()));

        Ok(file)
    }

    async fn get_dir(&self, _txn_id: &TxnId, _name: &PathSegment) -> TCResult<Option<Self>> {
        unimplemented!()
    }

    async fn get_file(&self, _txn_id: &TxnId, _name: &Id) -> TCResult<Option<Self::File>> {
        unimplemented!()
    }
}

pub async fn load(cache: Cache, mount_point: PathBuf) -> TCResult<Dir> {
    let contents = dir_contents(&mount_point).await?;
    Dir::load(cache, mount_point, contents).await
}
