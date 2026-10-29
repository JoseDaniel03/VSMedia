use std::cell::RefCell;
use leptos::prelude::*;
use wasm_bindgen::{JsCast,closure::Closure};

thread_local! {static TIMER: RefCell<Option<(i32,Closure<dyn FnMut()>)>> = const {RefCell::new(None)};}

pub fn stop(){
    TIMER.with(|t|if let Some((handle,_closure))=t.borrow_mut().take(){
        if let Some(w)=web_sys::window(){w.clear_interval_with_handle(handle);}
    });
}
pub fn start(speed: RwSignal<f64>){
    stop();
    let cb=Closure::new(move ||{
        if let Some(el)=web_sys::window().and_then(|w|w.document()).and_then(|d|d.get_element_by_id("script-reader")){
            if let Ok(html)=el.dyn_into::<web_sys::HtmlElement>() {
                html.set_scroll_top(html.scroll_top()+ (speed.get_untracked()/10.).round().max(1.) as i32);
            }
        }
    });
    if let Some(win)=web_sys::window(){
        if let Ok(handle)=win.set_interval_with_callback_and_timeout_and_arguments_0(cb.as_ref().unchecked_ref(),100){
            TIMER.with(|t|*t.borrow_mut()=Some((handle,cb)));
        }
    }
}
