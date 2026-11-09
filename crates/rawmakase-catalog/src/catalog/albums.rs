//! Making and arranging collections and collection sets: Lightroom's
//! collections (albums, as Lightroom for iPad calls them) and the sets that
//! hold them (its album folders). They are stored as a Lightroom import
//! stores them, by Lightroom's kinds, so the Collections panel shows the
//! ones made here as it shows imported ones. Photos are never touched:
//! deleting a collection or a set only takes its rows away.
use super::*;
use std::collections::HashMap;

const COLLECTION: &str = "com.adobe.ag.library.collection";
const SET: &str = "com.adobe.ag.library.group";

impl Catalog {
    /// Makes a collection, or with `set` a collection set, called `name`,
    /// inside set `parent` (at the top without one).
    pub fn create_collection(
        &mut self,
        name: &str,
        parent: Option<CollectionId>,
        set: bool,
    ) -> Result<CollectionId> {
        let name = checked_name(name)?;
        if let Some(parent) = parent {
            self.ensure_set(parent)?;
        }
        let kind = if set { SET } else { COLLECTION };
        self.db.write(|w| {
            w.insert_returning_id(
                sql!("INSERT INTO collections(name, parent, kind) VALUES (?, ?, ?) RETURNING id"),
                &[&name, &parent, &kind],
            )
        })
    }

    /// Gives collection or set `id` the name `name`.
    pub fn rename_collection(&mut self, id: CollectionId, name: &str) -> Result<()> {
        let name = checked_name(name)?;
        self.ensure_own(id)?;
        self.db.write(|w| {
            w.execute(
                sql!("UPDATE collections SET name=? WHERE id=?"),
                &[&name, &id],
            )?;
            Ok(())
        })
    }

    /// Puts collection or set `id` inside set `parent`, or at the top. A set
    /// can't go inside itself or a set it holds.
    pub fn move_collection(
        &mut self,
        id: CollectionId,
        parent: Option<CollectionId>,
    ) -> Result<()> {
        self.ensure_own(id)?;
        if let Some(parent) = parent {
            self.ensure_set(parent)?;
            let parents = self.collection_parents()?;
            let mut at = Some(parent);
            while let Some(p) = at {
                ensure!(
                    p != id,
                    "A folder can't go inside itself or a folder it holds"
                );
                at = parents.get(&p).copied().flatten();
            }
        }
        self.db.write(|w| {
            w.execute(
                sql!("UPDATE collections SET parent=? WHERE id=?"),
                &[&parent, &id],
            )?;
            Ok(())
        })
    }

    /// Deletes collection or set `id`; a set with everything inside it. The
    /// photos stay in the catalog.
    pub fn delete_collection(&mut self, id: CollectionId) -> Result<()> {
        self.ensure_own(id)?;
        let parents = self.collection_parents()?;
        let mut gone = vec![id];
        let mut i = 0;
        while i < gone.len() {
            let set = gone[i];
            for (child, parent) in &parents {
                if *parent == Some(set) && !gone.contains(child) {
                    gone.push(*child);
                }
            }
            i += 1;
        }
        self.db.write(|w| {
            for c in &gone {
                w.execute(
                    sql!("DELETE FROM collection_photos WHERE collection=?"),
                    &[c],
                )?;
                w.execute(sql!("DELETE FROM collections WHERE id=?"), &[c])?;
            }
            Ok(())
        })
    }

    /// Each collection's set, if any.
    fn collection_parents(&self) -> Result<HashMap<CollectionId, Option<CollectionId>>> {
        Ok(self
            .collections()?
            .into_iter()
            .map(|c| (c.id, c.parent))
            .collect())
    }

    fn kind_of(&self, id: CollectionId) -> Result<CollectionKind> {
        self.collections()?
            .into_iter()
            .find(|c| c.id == id)
            .map(|c| c.kind)
            .ok_or_else(|| anyhow::anyhow!("That album or folder isn't in the catalog"))
    }

    fn ensure_set(&self, id: CollectionId) -> Result<()> {
        ensure!(
            self.kind_of(id)? == CollectionKind::Set,
            "Albums and folders go inside folders only"
        );
        Ok(())
    }

    /// Lightroom's own (the Quick Collection) can't be renamed, moved or
    /// deleted.
    fn ensure_own(&self, id: CollectionId) -> Result<()> {
        ensure!(
            self.kind_of(id)? != CollectionKind::System,
            "The Quick Collection can't be changed"
        );
        Ok(())
    }
}

fn checked_name(name: &str) -> Result<&str> {
    let name = name.trim();
    ensure!(!name.is_empty(), "Name the album or folder");
    Ok(name)
}
