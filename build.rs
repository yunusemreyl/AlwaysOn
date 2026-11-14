fn main() {
    println!("cargo:rerun-if-changed=icons/awake.ico");
    println!("cargo:rerun-if-changed=icons/sleep.ico");
    println!("cargo:rerun-if-changed=build.rs");

    let mut res = winres::WindowsResource::new();
    res.set_icon("icons/awake.ico");
    res.append_rc_content("2 ICON \"icons/sleep.ico\"\n");

    res.set("ProductName", "AlwaysOn");
    res.set(
        "FileDescription",
        "AlwaysOn - Windows Sleep & Screen Lock Preventer",
    );
    res.set("CompanyName", "Yunus Emre Yilmaz (yunusemreyl)");
    res.set(
        "LegalCopyright",
        "Copyright (c) 2026 Yunus Emre Yilmaz. MIT License.",
    );
    res.set("OriginalFilename", "AlwaysOn.exe");
    res.set("InternalName", "AlwaysOn");

    res.set_manifest(
        r#"<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0" xmlns:asmv3="urn:schemas-microsoft-com:asm.v3">
  <assemblyIdentity version="1.1.0.0" processorArchitecture="*" name="yunusemreyl.AlwaysOn" type="win32"/>
  <description>AlwaysOn - Windows Sleep &amp; Screen Lock Preventer</description>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
    </application>
  </compatibility>
  <asmv3:application>
    <asmv3:windowsSettings>
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
    </asmv3:windowsSettings>
  </asmv3:application>
</assembly>"#,
    );

    res.compile().unwrap();
}
