//! Literal body insertion through the Office object model. Never uses
//! Application.ActiveDocument, which can belong to a different Word window.

use std::{mem::ManuallyDrop, ptr};
use windows::{
    Win32::{
        Foundation::HWND,
        System::{
            Com::{
                CLSIDFromProgID, DISPATCH_FLAGS, DISPATCH_METHOD, DISPATCH_PROPERTYGET, DISPPARAMS,
                IDispatch,
            },
            Ole::GetActiveObject,
            Variant::{VARIANT, VT_BOOL, VT_BSTR, VT_DISPATCH, VariantClear, VariantToInt32},
        },
        UI::{
            Accessibility::AccessibleObjectFromWindow,
            WindowsAndMessaging::{GA_ROOT, GetAncestor},
        },
    },
    core::{BSTR, GUID, IUnknown, Interface, PCWSTR, w},
};
use zeroize::Zeroize;

const OBJID_NATIVEOM: u32 = (-16i32) as u32;
const WD_SELECTION_IP: i32 = 1;
const WD_MAIN_TEXT_STORY: i32 = 1;
const WD_NO_PROTECTION: i32 = -1;
const OL_MAIL: i32 = 43;

struct Value(VARIANT);
impl Default for Value {
    fn default() -> Self {
        Self(VARIANT::default())
    }
}
impl Drop for Value {
    fn drop(&mut self) {
        let _ = unsafe { VariantClear(&mut self.0) };
    }
}
impl Value {
    fn dispatch(&self) -> Option<IDispatch> {
        unsafe {
            let data = &self.0.Anonymous.Anonymous;
            if data.vt != VT_DISPATCH {
                return None;
            }
            data.Anonymous.pdispVal.as_ref().cloned()
        }
    }
    fn boolean(&self) -> Option<bool> {
        unsafe {
            let data = &self.0.Anonymous.Anonymous;
            (data.vt == VT_BOOL).then(|| data.Anonymous.boolVal.0 != 0)
        }
    }
    fn int(&self) -> Option<i32> {
        unsafe { VariantToInt32(&self.0) }.ok()
    }
}

struct TextArg(Value);
impl TextArg {
    fn new(text: &[u16]) -> Option<Self> {
        let string = BSTR::from_wide(text);
        if text.len() != string.len() {
            return None;
        }
        let mut value = Value::default();
        unsafe {
            let data = &mut *value.0.Anonymous.Anonymous;
            data.vt = VT_BSTR;
            data.Anonymous.bstrVal = ManuallyDrop::new(string);
        }
        Some(Self(value))
    }
}
impl Drop for TextArg {
    fn drop(&mut self) {
        // This BSTR is exclusively owned here. Word may have its own COM copy.
        unsafe {
            let string = &self.0.0.Anonymous.Anonymous.Anonymous.bstrVal;
            if !string.is_empty() {
                std::slice::from_raw_parts_mut(string.as_ptr().cast_mut(), string.len()).zeroize();
            }
        }
    }
}

fn name_id(object: &IDispatch, member: PCWSTR) -> Option<i32> {
    let mut id = 0;
    unsafe { object.GetIDsOfNames(&GUID::zeroed(), &member, 1, 0, &mut id) }.ok()?;
    Some(id)
}

fn invoke(
    object: &IDispatch,
    id: i32,
    kind: DISPATCH_FLAGS,
    args: &mut [VARIANT],
) -> windows::core::Result<Value> {
    let params = DISPPARAMS {
        rgvarg: if args.is_empty() {
            ptr::null_mut()
        } else {
            args.as_mut_ptr()
        },
        cArgs: args.len() as u32,
        ..Default::default()
    };
    let mut result = Value::default();
    unsafe {
        object.Invoke(
            id,
            &GUID::zeroed(),
            0,
            kind,
            &params,
            Some(&mut result.0),
            None,
            None,
        )
    }?;
    Ok(result)
}

fn prop(object: &IDispatch, member: PCWSTR) -> Option<Value> {
    invoke(
        object,
        name_id(object, member)?,
        DISPATCH_PROPERTYGET,
        &mut [],
    )
    .ok()
}

fn native_window(body: HWND) -> Option<IDispatch> {
    let mut raw = ptr::null_mut();
    unsafe { AccessibleObjectFromWindow(body, OBJID_NATIVEOM, &IDispatch::IID, &mut raw) }.ok()?;
    if raw.is_null() {
        return None;
    }
    Some(unsafe { IDispatch::from_raw(raw) })
}

fn same_object(left: &IDispatch, right: &IDispatch) -> bool {
    match (left.cast::<IUnknown>(), right.cast::<IUnknown>()) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

fn inspector_document() -> Option<IDispatch> {
    // Never launches Outlook or creates, saves, or sends an item.
    let clsid = unsafe { CLSIDFromProgID(w!("Outlook.Application")) }.ok()?;
    let mut object = None;
    unsafe { GetActiveObject(&clsid, None, &mut object) }.ok()?;
    let app = object?.cast::<IDispatch>().ok()?;
    let inspector = prop(&app, w!("ActiveInspector"))?.dispatch()?;
    let item = prop(&inspector, w!("CurrentItem"))?.dispatch()?;
    if prop(&item, w!("Class"))?.int()? != OL_MAIL || prop(&item, w!("Sent"))?.boolean()? {
        return None;
    }
    let is_word_mail = name_id(&inspector, w!("IsWordMail"))?;
    if !invoke(&inspector, is_word_mail, DISPATCH_METHOD, &mut [])
        .ok()?
        .boolean()?
    {
        return None;
    }
    prop(&inspector, w!("WordEditor"))?.dispatch()
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum OfficeKind {
    Word,
    Outlook,
}

pub(super) struct BodyCaret {
    kind: OfficeKind,
    document: IDispatch,
    index: i32,
}

impl BodyCaret {
    pub(super) fn same(&self, other: &Self) -> bool {
        self.kind == other.kind
            && self.index == other.index
            && same_object(&self.document, &other.document)
    }
}

pub(super) fn caret(
    kind: OfficeKind,
    root: HWND,
    focused_body: HWND,
) -> Option<(BodyCaret, IDispatch)> {
    let window = native_window(focused_body)?;
    let hwnd = HWND(prop(&window, w!("Hwnd"))?.int()? as u32 as usize as _);
    if unsafe { GetAncestor(hwnd, GA_ROOT) } != root {
        return None;
    }
    let document = prop(&window, w!("Document"))?.dispatch()?;
    if kind == OfficeKind::Outlook && !same_object(&document, &inspector_document()?) {
        return None;
    }
    if prop(&document, w!("ReadOnly"))?.boolean()?
        || prop(&document, w!("ProtectionType"))?.int()? != WD_NO_PROTECTION
    {
        return None;
    }
    let selection = prop(&window, w!("Selection"))?.dispatch()?;
    if prop(&selection, w!("Type"))?.int()? != WD_SELECTION_IP {
        return None;
    }
    let range = prop(&selection, w!("Range"))?.dispatch()?;
    let index = prop(&range, w!("Start"))?.int()?;
    if index < 0
        || index != prop(&range, w!("End"))?.int()?
        || prop(&range, w!("StoryType"))?.int()? != WD_MAIN_TEXT_STORY
    {
        return None;
    }
    Some((
        BodyCaret {
            kind,
            document,
            index,
        },
        range,
    ))
}

pub(super) enum Write {
    Delivered,
    NotStarted,
    Uncertain,
}

pub(super) fn write(
    saved: &BodyCaret,
    root: HWND,
    body: HWND,
    text: &[u16],
    begin: impl FnOnce() -> bool,
) -> Write {
    let Some((now, range)) = caret(saved.kind, root, body) else {
        return Write::NotStarted;
    };
    if !saved.same(&now) {
        return Write::NotStarted;
    }
    let Some(insert) = name_id(&range, w!("InsertAfter")) else {
        return Write::NotStarted;
    };
    let Some(mut arg) = TextArg::new(text) else {
        return Write::NotStarted;
    };
    // The range is anchored in this one document, not the global selection.
    // One call supplies the entire literal and produces one native undo action.
    if !begin() {
        return Write::NotStarted;
    }
    match invoke(
        &range,
        insert,
        DISPATCH_METHOD,
        std::slice::from_mut(&mut arg.0.0),
    ) {
        Ok(_) => Write::Delivered,
        Err(_) => Write::Uncertain,
    }
}
