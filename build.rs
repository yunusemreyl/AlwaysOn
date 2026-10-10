fn main() {
    let mut res = winres::WindowsResource::new();
    res.set_icon("icons/awake.ico");
    res.append_rc_content("2 ICON \"icons/sleep.ico\"\n");
    res.compile().unwrap();
}
