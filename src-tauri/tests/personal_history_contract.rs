//! All databases and files in this suite belong to its isolated run.
use pinfeather_lib::store::{Store, Snapshot, Payload, SourceConfidence};
use std::path::PathBuf;
fn root() -> PathBuf { let p=std::env::temp_dir().join(format!("pf-personal-{}",uuid::Uuid::new_v4())); std::fs::create_dir(&p).unwrap(); p }
fn add(s:&mut Store,id:&str)->i64 { s.insert(&Snapshot { capture_id:id.into(), captured_at_ms:123,source_process:Some(r"C:\Synthetic\editor.exe".into()),source_confidence:SourceConfidence::Owner,omitted_formats:vec![],payloads:vec![Payload{format_name:"CF_UNICODETEXT".into(),data:"样本".encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect()},Payload{format_name:"HTML Format".into(),data:b"<b>sample</b>".to_vec()}] }).unwrap().clip_id }
#[test]
fn clear_is_transactional_and_cascades_payloads() { let mut s=Store::open_in_memory().unwrap(); add(&mut s,"a"); add(&mut s,"b"); assert_eq!(s.clear_history().unwrap(),2);assert_eq!(s.usage().unwrap().items,0); assert_eq!(s.usage().unwrap().payload_bytes,0); assert_eq!(s.clear_history().unwrap(),0); let id=add(&mut s,"c");assert!(id>2); }
#[test]
fn backup_restore_preserves_all_stored_fields_and_never_overwrites() { let p=root();let path=p.join("original.sqlite3");let mut s=Store::open(&path).unwrap();let a=add(&mut s,"a");let b=add(&mut s,"b");let backup=p.join("backup.sqlite3");s.backup_to(&backup).unwrap();let bytes=std::fs::read(&backup).unwrap();assert!(s.backup_to(&backup).is_err());assert_eq!(std::fs::read(&backup).unwrap(),bytes);assert!(s.delete(a).unwrap());let dest=p.join("restored");Store::restore_to_new_directory(&backup,&dest).unwrap();assert_eq!(std::fs::read(&backup).unwrap(),bytes);let restored=Store::open(&dest.join("history.sqlite3")).unwrap();assert_eq!(restored.usage().unwrap().items,2);assert_eq!(restored.get(b).unwrap(),s.get(b).unwrap());assert_eq!(restored.get(a).unwrap().unwrap().text_full.as_deref(),Some("样本"));assert!(Store::restore_to_new_directory(&backup,&dest).is_err());assert_eq!(restored.usage().unwrap().items,2); }
#[test]
fn restore_rejects_unknown_and_existing_empty_target_before_creation() {let p=root();let unknown=p.join("unknown.sqlite3");let conn=rusqlite::Connection::open(&unknown).unwrap();conn.execute_batch("CREATE TABLE stranger(value TEXT)").unwrap();drop(conn);let before=std::fs::read(&unknown).unwrap();let dest=p.join("rejected");assert!(Store::restore_to_new_directory(&unknown,&dest).is_err());assert!(!dest.exists());assert_eq!(std::fs::read(&unknown).unwrap(),before);let empty=p.join("existing");std::fs::create_dir(&empty).unwrap();assert!(Store::restore_to_new_directory(&unknown,&empty).is_err());assert!(empty.exists());}

#[test]
fn clear_failure_rolls_back_and_reopening_checks_payload_integrity() {
 let p=root();let path=p.join("history.sqlite3");let mut s=Store::open(&path).unwrap();let id=add(&mut s,"a");add(&mut s,"b");
 let conn=rusqlite::Connection::open(&path).unwrap();conn.execute_batch("CREATE TRIGGER fail_delete BEFORE DELETE ON clips WHEN OLD.capture_id='b' BEGIN SELECT RAISE(ABORT,'synthetic failure'); END;").unwrap();
 assert!(s.clear_history().is_err());assert_eq!(s.usage().unwrap().items,2);assert!(s.get(id).unwrap().is_some());
 conn.execute_batch("DROP TRIGGER fail_delete").unwrap();drop(conn);assert_eq!(s.clear_history().unwrap(),2);drop(s);
 let reopened=Store::open(&path).unwrap();assert_eq!(reopened.usage().unwrap().payload_bytes,0);
}
#[test]
fn backup_refuses_empty_existing_file_and_unknown_version_restore_keeps_bytes() {
 let p=root();let mut s=Store::open_in_memory().unwrap();add(&mut s,"a");let target=p.join("existing.sqlite3");std::fs::write(&target,[]).unwrap();assert!(s.backup_to(&target).is_err());assert_eq!(std::fs::metadata(&target).unwrap().len(),0);
 let backup=p.join("backup.sqlite3");s.backup_to(&backup).unwrap();let conn=rusqlite::Connection::open(&backup).unwrap();conn.pragma_update(None,"user_version",99).unwrap();drop(conn);let before=std::fs::read(&backup).unwrap();let dest=p.join("unknown-version");assert!(Store::restore_to_new_directory(&backup,&dest).is_err());assert!(!dest.exists());assert_eq!(std::fs::read(&backup).unwrap(),before);
}

#[test]
fn offline_restore_executable_uses_new_directory_and_reports_exit_status() {
 let p=root();let mut s=Store::open_in_memory().unwrap();add(&mut s,"cli");let backup=p.join("backup.sqlite3");s.backup_to(&backup).unwrap();let before=std::fs::read(&backup).unwrap();let destination=p.join("cli-restored");
 let output=std::process::Command::new(env!("CARGO_BIN_EXE_history_restore")).arg(&backup).arg(&destination).output().unwrap();assert!(output.status.success(),"{:?}",output);
 assert_eq!(Store::open(&destination.join("history.sqlite3")).unwrap().usage().unwrap().items,1);
 assert_eq!(std::fs::read(&backup).unwrap(),before);
 assert_eq!(std::process::Command::new(env!("CARGO_BIN_EXE_history_restore")).arg(&backup).arg(&destination).output().unwrap().status.code(),Some(1));
 assert_eq!(std::process::Command::new(env!("CARGO_BIN_EXE_history_restore")).output().unwrap().status.code(),Some(2));
}
