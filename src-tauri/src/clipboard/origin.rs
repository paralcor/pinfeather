//! Internal writes are identified by registration, owner and completed sequence,
//! never by payload equality or an elapsed suppression window.
use std::collections::VecDeque;
use crate::contracts::{ContractError,ContractResult,ErrorCode};

pub const MAX_OPERATIONS:usize=128;
#[derive(Debug,Clone,Copy,PartialEq,Eq)]
pub enum Classification {External,Pending,Internal}
struct Operation {counter:u64,sequence:Option<u32>,finished:bool,associated:bool}
pub struct OriginRegistry {instance:[u8;16],owner:usize,next:u64,operations:VecDeque<Operation>,current:Option<u64>}
impl OriginRegistry {
    pub fn new(instance:[u8;16],owner:usize)->Self{Self{instance,owner,next:0,operations:VecDeque::new(),current:None}}
    pub fn set_owner(&mut self,owner:usize){self.owner=owner;}
    pub fn begin(&mut self)->ContractResult<(u64,[u8;28])>{
        if self.owner==0{return Err(ContractError::new(ErrorCode::InjectionFailed,"clipboard owner unavailable"));}
        if self.operations.len()>=MAX_OPERATIONS {
            let removable=self.operations.iter().position(|op|op.associated&&Some(op.counter)!=self.current).ok_or_else(||ContractError::new(ErrorCode::Busy,"internal operation registry is full"))?;
            self.operations.remove(removable);
        }
        self.next=self.next.checked_add(1).ok_or_else(||ContractError::new(ErrorCode::Busy,"internal operation identity exhausted"))?;
        self.operations.push_back(Operation{counter:self.next,sequence:None,finished:false,associated:false});
        let mut marker=[0u8;28];marker[..4].copy_from_slice(&1u32.to_le_bytes());marker[4..20].copy_from_slice(&self.instance);marker[20..].copy_from_slice(&self.next.to_le_bytes());
        Ok((self.next,marker))
    }
    pub fn finish(&mut self,counter:u64,sequence:Option<u32>){if let Some(op)=self.operations.iter_mut().find(|o|o.counter==counter){op.finished=true;op.sequence=sequence.filter(|s|*s!=0);op.associated=op.sequence.is_none();if op.sequence.is_some(){self.current=Some(counter);}}}
    pub fn classify(&mut self,raw:Option<&[u8]>,owner:Option<usize>,sequence:u32)->Classification{
        if sequence==0{return Classification::External;}
        if self.current.is_some_and(|id|self.operations.iter().any(|op|op.counter==id&&op.sequence!=Some(sequence))){self.current=None;}
        let Some(raw)=raw else{return Classification::External;};
        if !(28..=256).contains(&raw.len())||raw[..4]!=1u32.to_le_bytes()||raw[4..20]!=self.instance||self.owner==0||owner!=Some(self.owner){return Classification::External;}
        let counter=u64::from_le_bytes(raw[20..28].try_into().unwrap());
        let Some(op)=self.operations.iter_mut().find(|o|o.counter==counter) else{return Classification::External;};
        if !op.finished{return Classification::Pending;}
        if op.sequence!=Some(sequence){return Classification::External;}
        op.associated=true;self.current=Some(counter);Classification::Internal
    }
    pub fn len(&self)->usize{self.operations.len()}
}

pub trait WriteApi {
    fn open(&mut self,owner:usize)->ContractResult<()>;
    fn empty(&mut self)->ContractResult<()>;
    fn allocate(&mut self,bytes:&[u8])->ContractResult<usize>;
    /// On success the system owns the handle. On failure the caller still owns it.
    fn set(&mut self,name:&str,handle:usize)->ContractResult<()>;
    fn free(&mut self,handle:usize);
    fn close(&mut self)->ContractResult<()>;
    fn sequence(&mut self)->u32;
}

pub fn write_text(api:&mut impl WriteApi,registry:&mut OriginRegistry,text:&[u8])->ContractResult<u64>{
    super::capture::validate_payloads(&[crate::store::Payload{format_name:"CF_UNICODETEXT".into(),data:text.to_vec()}]).map_err(|_|ContractError::invalid_input("invalid clipboard text"))?;
    let(counter,marker)=registry.begin()?;
    if let Err(error)=api.open(registry.owner){registry.finish(counter,None);return Err(error);}
    let mut marker_set=false;
    let result=(||{
        api.empty()?;
        for (name,bytes) in [(super::capture::INTERNAL,marker.as_slice()),(super::capture::HISTORY,&[0u8;4]),(super::capture::CLOUD,&[0u8;4]),("CF_UNICODETEXT",text)] {
            let handle=api.allocate(bytes)?;
            if let Err(error)=api.set(name,handle){api.free(handle);return Err(error);}
            if name==super::capture::INTERNAL{marker_set=true;}
        }
        Ok(())
    })();
    let closed=api.close();
    registry.finish(counter,if marker_set{Some(api.sequence())}else{None});
    result?;closed?;Ok(counter)
}
