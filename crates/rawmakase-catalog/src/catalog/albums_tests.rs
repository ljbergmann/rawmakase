//! Albums and their folders (collections and collection sets, `albums`).
use super::*;

fn catalog() -> Result<(tempfile::TempDir, Catalog, Vec<PhotoId>)> {
    let dir = tempfile::tempdir()?;
    let folder = dir.path().join("photos");
    std::fs::create_dir(&folder)?;
    for name in ["a.dng", "b.dng"] {
        std::fs::write(folder.join(name), format!("synthetic {name}"))?;
    }
    let mut cat = Catalog::create(&dir.path().join("Photos.rawmakase"))?;
    cat.add_folder(&folder)?;
    let ids = cat.photos()?.iter().map(|p| p.id).collect();
    Ok((dir, cat, ids))
}

fn find(cat: &Catalog, id: CollectionId) -> Collection {
    cat.collections()
        .unwrap()
        .into_iter()
        .find(|c| c.id == id)
        .unwrap()
}

/// Folders hold albums and folders; albums hold photos. Both read back with
/// Lightroom's kinds, as an imported catalog's do.
#[test]
fn albums_go_in_folders_and_hold_photos() -> Result<()> {
    let (_dir, mut cat, photos) = catalog()?;
    let trips = cat.create_collection(" Trips ", None, true)?;
    let japan = cat.create_collection("Japan", Some(trips), false)?;
    assert_eq!(find(&cat, trips).kind, CollectionKind::Set);
    assert_eq!(find(&cat, trips).name, "Trips");
    assert_eq!(find(&cat, japan).kind, CollectionKind::Collection);
    assert_eq!(find(&cat, japan).parent, Some(trips));
    cat.change_collection(japan, &photos, &[])?;
    assert_eq!(cat.collection_members(japan)?.len(), 2);

    // Only folders take albums, and names aren't empty.
    assert!(
        cat.create_collection("Inside an album", Some(japan), false)
            .is_err()
    );
    assert!(cat.create_collection("  ", None, false).is_err());
    Ok(())
}

/// Renaming and moving change only the album or folder; a folder can't go
/// inside itself or a folder it holds.
#[test]
fn albums_and_folders_are_renamed_and_moved() -> Result<()> {
    let (_dir, mut cat, _) = catalog()?;
    let outer = cat.create_collection("2026", None, true)?;
    let inner = cat.create_collection("Summer", Some(outer), true)?;
    let album = cat.create_collection("Beach", None, false)?;
    cat.rename_collection(album, "Beach days")?;
    assert_eq!(find(&cat, album).name, "Beach days");
    cat.move_collection(album, Some(inner))?;
    assert_eq!(find(&cat, album).parent, Some(inner));
    assert!(
        cat.move_collection(outer, Some(inner)).is_err(),
        "into its own folder"
    );
    assert!(
        cat.move_collection(outer, Some(outer)).is_err(),
        "into itself"
    );
    cat.move_collection(album, None)?;
    assert_eq!(find(&cat, album).parent, None);
    Ok(())
}

/// Deleting a folder takes what is inside it too; photos stay; the Quick
/// Collection can't be renamed, moved or deleted.
#[test]
fn deleting_a_folder_takes_its_albums_but_never_photos() -> Result<()> {
    let (_dir, mut cat, photos) = catalog()?;
    let folder = cat.create_collection("Old", None, true)?;
    let sub = cat.create_collection("Older", Some(folder), true)?;
    let album = cat.create_collection("Gone", Some(sub), false)?;
    let kept = cat.create_collection("Kept", None, false)?;
    cat.change_collection(album, &photos, &[])?;
    cat.delete_collection(folder)?;
    let left: Vec<CollectionId> = cat.collections()?.iter().map(|c| c.id).collect();
    assert!(!left.contains(&folder) && !left.contains(&sub) && !left.contains(&album));
    assert!(left.contains(&kept));
    assert_eq!(cat.photos()?.len(), 2);
    assert!(cat.collection_members(album)?.is_empty());

    let quick = cat.quick_collection()?;
    assert!(cat.rename_collection(quick, "Mine").is_err());
    assert!(cat.delete_collection(quick).is_err());
    Ok(())
}
