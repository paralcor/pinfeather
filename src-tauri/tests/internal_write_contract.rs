//! Same production write transaction and origin rules with synthetic Win32 handles.
use pinfeather_lib::{clipboard::origin::*,contracts::{ContractError,ContractResult,ErrorCode}};
use std::collections::BTreeMap;

#[test]
fn marker_requires_registered_instance_owner_and_completed_sequence(){
    let mut registry=OriginRegistry::new([7;16],99);let (id,marker)=registry.begin().unwrap();assert_eq!(marker.len(),28);assert_eq!(&marker[..4],&1u32.to_le_bytes());
    assert_eq!(registry.classify(Some(&marker),Some(99),10),Classification::Pending);registry.finish(id,Some(10));
    for (owner,seq) in [(Some(98),10),(Some(99),11),(None,10),(Some(99),0)]{assert_eq!(registry.classify(Some(&marker),owner,seq),Classification::External);}
    assert_eq!(registry.classify(Some(&marker),Some(99),10),Classification::Internal);
    let mut padding=marker.to_vec();padding.extend([0;10]);assert_eq!(registry.classify(Some(&padding),Some(99),10),Classification::Internal);
    for bytes in [vec![0;27],vec![0;257],vec![0;28]]{assert_eq!(registry.classify(Some(&bytes),Some(99),10),Classification::External);}
    let mut other=OriginRegistry::new([8;16],99);assert_eq!(other.classify(Some(&marker),Some(99),10),Classification::External);
}

#[test]
fn registry_never_evicts_current_or_unassociated_operations(){
    let mut registry=OriginRegistry::new([1;16],99);
    for _ in 0..128 {registry.begin().unwrap();}assert!(registry.begin().is_err());assert_eq!(registry.len(),128);
    let mut registry=OriginRegistry::new([1;16],99);let mut first=None;
    for seq in 1..=200 {let(id,marker)=registry.begin().unwrap();registry.finish(id,Some(seq));assert_eq!(registry.classify(Some(&marker),Some(99),seq),Classification::Internal);if seq==1{first=Some(marker);}}
    assert_eq!(registry.len(),128);assert_eq!(registry.classify(first.as_ref().map(|b|b.as_slice()),Some(99),1),Classification::External);
}

struct Writer {fail:usize,step:usize,next:usize,owned:BTreeMap<usize,Vec<u8>>,transferred:Vec<(String,Vec<u8>)>,freed:usize,closed:bool}
impl Writer {fn new(fail:usize)->Self{Self{fail,step:0,next:0,owned:BTreeMap::new(),transferred:vec![],freed:0,closed:false}}fn check(&mut self)->ContractResult<()>{self.step+=1;if self.step==self.fail{Err(ContractError::new(ErrorCode::InjectionFailed,"synthetic write failure"))}else{Ok(())}}}
impl WriteApi for Writer {
    fn open(&mut self,owner:usize)->ContractResult<()>{assert_eq!(owner,99);self.check()}
    fn empty(&mut self)->ContractResult<()>{self.check()}
    fn allocate(&mut self,bytes:&[u8])->ContractResult<usize>{self.check()?;self.next+=1;self.owned.insert(self.next,bytes.to_vec());Ok(self.next)}
    fn set(&mut self,name:&str,handle:usize)->ContractResult<()>{self.check()?;self.transferred.push((name.into(),self.owned.remove(&handle).unwrap()));Ok(())}
    fn free(&mut self,handle:usize){assert!(self.owned.remove(&handle).is_some());self.freed+=1;}
    fn close(&mut self)->ContractResult<()>{self.closed=true;self.check()}
    fn sequence(&mut self)->u32{50}
}

#[test]
fn internal_write_orders_markers_before_text_and_transfers_handles_once(){
    let mut registry=OriginRegistry::new([1;16],99);let mut api=Writer::new(0);let id=write_text(&mut api,&mut registry,&[65,0,0,0]).unwrap();
    assert_eq!(id,1);assert_eq!(api.transferred.iter().map(|(n,_)|n.as_str()).collect::<Vec<_>>(),vec!["Pinfeather.InternalOrigin.v1","CanIncludeInClipboardHistory","CanUploadToCloudClipboard","CF_UNICODETEXT"]);assert!(api.owned.is_empty());assert!(api.closed);
    assert_eq!(registry.classify(Some(&api.transferred[0].1),Some(99),50),Classification::Internal);
}

#[test]
fn each_failure_frees_only_untransferred_memory_and_never_reports_success(){
    for fail in 1..=11 {let mut registry=OriginRegistry::new([1;16],99);let mut api=Writer::new(fail);assert!(write_text(&mut api,&mut registry,&[65,0,0,0]).is_err(),"step {fail}");assert!(api.owned.is_empty(),"step {fail}");assert_eq!(api.closed,fail!=1,"step {fail}");if fail<10{assert!(!api.transferred.iter().any(|(n,_)|n=="CF_UNICODETEXT"));}}
}
