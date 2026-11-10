# Neramit Cut

โปรแกรมจัดแผนตัดวัสดุเส้นสำหรับ Windows โดย **BiGByte Studio** รองรับภาษาไทยและ English ใช้งานจัดแผนตัดแบบออฟไลน์ และเชื่อมต่อ GitHub เมื่อตรวจหาอัปเดต

## ดาวน์โหลดและติดตั้ง

ดาวน์โหลดจาก [GitHub Releases เวอร์ชันล่าสุด](https://github.com/BiG-byt3/Neramit-Cut/releases/latest)

- **ชุดติดตั้ง:** เลือก `NeramitCut-Setup-<version>.exe` ติดตั้งสำหรับผู้ใช้ Windows ปัจจุบัน ไม่ต้องใช้สิทธิ์ผู้ดูแลระบบ
- **Portable:** เลือก `NeramitCut-<version>.exe` และเปิดใช้งานได้โดยตรง

## การใช้งาน

- ป้อนรายการชิ้นงานและวัสดุตั้งต้น พร้อมความยาว จำนวน ชนิดวัสดุ และข้อมูลประกอบ
- คำนวณแผนตัดและดูผลการใช้วัสดุ
- เปิดและบันทึกโครงการ XML นำเข้าตาราง Excel/CSV และส่งออกผลเป็น Excel/CSV หรือ DXF
- สลับส่วนติดต่อภาษาไทยและ English

ตรวจสอบแผนและขนาดจริงก่อนตัดวัสดุเสมอ

## อัปเดตผ่านออนไลน์

เปิด **Help > Check for updates…** หรือปุ่ม **Check for updates** ในหน้าต่าง About แล้วเลือกดาวน์โหลดและติดตั้ง โปรแกรมตรวจ SHA-256 และลายเซ็น ECDSA ของไฟล์ก่อนใช้งาน สามารถเลือกอัปเดตทันทีและเปิดโปรแกรมใหม่ หรืออัปเดตเมื่อปิดโปรแกรม

ทั้งชุดติดตั้งและ Portable รองรับการอัปเดต โดยเก็บงาน วัสดุ และการตั้งค่าเดิมไว้ รุ่นที่ไม่มีระบบตรวจอัปเดตต้องดาวน์โหลดและติดตั้งด้วยตนเองก่อนหนึ่งครั้ง

## Build from source (Windows x64)

Install Rust via rustup and MSYS2 with the `mingw-w64-x86_64-gcc` package in the MINGW64 environment. Use a checkout path without spaces, for example `C:\src\Neramit-Cut`, because the resource compiler cannot handle all paths containing spaces. In PowerShell, from the repository root:

```powershell
rustup toolchain install stable-x86_64-pc-windows-gnu --profile minimal
$env:RUSTUP_TOOLCHAIN = 'stable-x86_64-pc-windows-gnu'
$env:PATH = "C:\msys64\mingw64\bin;$env:PATH"
$env:WINDRES = 'C:\msys64\mingw64\bin\windres.exe'
cargo test --locked
cargo build --release --locked
cargo test --manifest-path installer/Cargo.toml --locked
cargo build --manifest-path installer/Cargo.toml --release --locked
```

Adjust the MSYS2 path to match your installation. Build the application before the installer: the installer embeds `target/release/NeramitCut.exe`. Outputs are `target/release/NeramitCut.exe` and `installer/target/release/NeramitCut-Setup.exe`. The bundled fonts, translations and icon are embedded in the executables.

GitHub Actions builds and tests both executables. Its CI artifacts are development builds and have no signed update manifest. For signed releases, see [docs/releasing.md](docs/releasing.md).

## เงื่อนไขการใช้งาน

ใช้งานและแจกจ่ายโปรแกรมตาม [ข้อตกลงภาษาไทย](installer/license-th.txt) / [English agreement](installer/license-en.txt) ของ BiGByte Studio ส่วนประกอบโอเพนซอร์สและฟอนต์มีเงื่อนไขของตนเอง ดูในหน้าต่าง About และ [ใบอนุญาตฟอนต์](assets/fonts/OFL.txt)

