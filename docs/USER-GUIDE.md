# คู่มือการติดตั้งและใช้งาน LocalFlow (ภาษาไทย)

LocalFlow คือโปรแกรมพิมพ์ด้วยเสียงแบบ system-wide (แบบเดียวกับ Wispr Flow) ที่ประมวลผล **ในเครื่องคุณ 100%** — ไม่มี cloud, ไม่ต้องสมัครสมาชิก, ไม่ส่งข้อมูลออกจากเครื่อง

กดปุ่มลัดค้างไว้ที่แอปไหนก็ได้ → พูด → ปล่อยปุ่ม → ข้อความที่ AI เกลาแล้ว (ตัดคำอุทาน, ใส่วรรคตอน, แก้คำพูดซ้ำ/พูดผิดแล้วแก้) จะถูกพิมพ์ลงช่องข้อความที่เคอร์เซอร์อยู่ทันที

---

## 1. ดาวน์โหลดตัวติดตั้ง

ไปที่ https://github.com/p2544/localflow

**วิธีที่ 1 — จาก Releases (แนะนำ):** แท็บ **Releases** ด้านขวาของหน้า repo → ดาวน์โหลดไฟล์ของ OS คุณ

**วิธีที่ 2 — จาก CI ล่าสุด:** แท็บ **Actions** → เลือก run ล่าสุดที่เป็นสีเขียว → เลื่อนลงไปที่หัวข้อ **Artifacts**

| ระบบ | ไฟล์ที่ใช้ |
|---|---|
| Windows 11 (x64) | `LocalFlow_0.1.2_x64-setup.exe` (NSIS) หรือ `.msi` |
| macOS Apple Silicon (M1–M4) | `LocalFlow_0.1.2_aarch64.dmg` |
| macOS Intel | build จาก source (ดูข้อ 7 — GitHub เลิกให้บริการ Intel runner แล้ว) |

## 2. ติดตั้ง

### Windows 11
1. ดับเบิลคลิก `LocalFlow_..._x64-setup.exe`
2. ถ้า Windows SmartScreen เตือน "Windows protected your PC" (เพราะแอปยังไม่ได้ซื้อ code-signing certificate) → คลิก **More info** → **Run anyway**
3. กด Next ตามตัวติดตั้งจนเสร็จ — ไอคอน LocalFlow จะปรากฏใน system tray (มุมขวาล่าง)

### macOS 12+
1. เปิดไฟล์ `.dmg` แล้วลาก **LocalFlow** ไปใส่โฟลเดอร์ **Applications**
2. **สำคัญ:** ถ้าเปิดแล้วขึ้นว่า *"LocalFlow is damaged and can't be opened. You should move it to the Trash"* — ไฟล์ไม่ได้เสีย นี่คือ Gatekeeper บล็อกแอปที่ยังไม่ได้ notarize กับ Apple แก้โดยเปิด **Terminal** (⌘+Space พิมพ์ Terminal) แล้วรันคำสั่งนี้ครั้งเดียว:
   ```
   xattr -cr /Applications/LocalFlow.app
   ```
   จากนั้นเปิดแอปได้ตามปกติ
3. ถ้าไม่เจอ "damaged" แต่เตือนว่า unidentified developer: คลิกขวาที่แอป → **Open** → **Open** หรือ System Settings → Privacy & Security → กด **Open Anyway**
3. อนุญาตสิทธิ์ 3 อย่างเมื่อระบบถาม (ตัว onboarding ในแอปมีปุ่มพาไปหน้า Settings ที่ถูกต้องให้เลย):
   - **Microphone** — เพื่ออัดเสียง
   - **Accessibility** — เพื่อพิมพ์ข้อความลงแอปอื่น
   - **Input Monitoring** — เพื่อให้ปุ่มลัดทำงานทุกแอป

## 3. ตั้งค่าครั้งแรก (Onboarding — ประมาณ 2 นาที + เวลาดาวน์โหลดโมเดล)

เปิดแอปครั้งแรกจะเจอ wizard 4 ขั้น:

1. **Welcome** — กด Get started
2. **Permissions** — (macOS) กดปุ่มเปิด System Settings แล้วติ๊กอนุญาต LocalFlow
3. **Download AI models** — กดดาวน์โหลด 2 โมเดล (เก็บในเครื่อง ใช้ครั้งเดียว):
   - **Whisper Small** (~466 MB) — แปลงเสียงเป็นข้อความ (แนะนำขั้นต่ำสำหรับภาษาไทย)
   - **Qwen2.5 3B** (~2 GB) — AI เกลาข้อความ (ตัดคำอุทาน, วรรคตอน, แก้ backtracking)
4. **Try it** — กด Record ทดสอบพูด แล้วกด Finish setup

## 4. วิธีใช้งานประจำวัน

- **กดค้าง `Ctrl+Shift+Space`** (ค่าเริ่มต้น — เปลี่ยนได้ใน Settings) → พูด → **ปล่อยปุ่ม**
- แคปซูลเล็ก ๆ (pill) จะโผล่กลางจอด้านล่างแสดงคลื่นเสียงขณะอัด และสถานะ Transcribing → Polishing
- ข้อความจะถูกพิมพ์ลงช่องที่เคอร์เซอร์อยู่ ไม่ว่าจะเป็น LINE, Word, Chrome, VS Code, Slack ฯลฯ

ตัวอย่างที่ AI จัดการให้:
- "เอ่อ พรุ่งนี้ประชุมบ่ายสอง อ๊ะไม่สิ บ่ายสาม" → **"พรุ่งนี้ประชุมบ่ายสาม"**
- "um so let's meet at five pm no wait six pm" → **"Let's meet at 6pm."**
- "ต้องซื้อสามอย่าง หนึ่งนม สองไข่ สามกาแฟ" → รายการแบบมีเลขข้อ

### หน้าต่างหลัก (คลิกไอคอนใน tray/menu bar)
| เมนู | ทำอะไร |
|---|---|
| **Settings** | เปลี่ยนปุ่มลัด, โหมด (กดค้าง / กดสลับ hands-free), ภาษา (ไทย/อังกฤษ/100+), ไมค์, เปิด-ปิด AI cleanup, วิธีพิมพ์ (Paste/Type), เปิดตอน login, low-memory mode |
| **Models** | ดาวน์โหลด/สลับ/ลบโมเดล (Whisper Base เร็ว / Small สมดุล / Large-v3-Turbo แม่นสุด) |
| **Dictionary** | เพิ่มชื่อคน/ศัพท์เฉพาะ ให้ระบบรู้จักและไม่ถูก AI "แก้" |
| **History** | ประวัติการพูดทั้งหมด (เก็บในเครื่องเท่านั้น) ค้นหา/คัดลอก/ลบ/ปิดได้ |
| **Scratchpad** | พูดใส่หน้าต่างแอปเองโดยไม่ต้องมีช่องข้อความแอปอื่น |
| **Latency** | ดูเวลาแต่ละขั้น (อัด/VAD/ASR/LLM/inject) ของครั้งล่าสุด |

## 5. เรื่องความเป็นส่วนตัว

- เสียงและข้อความ**ไม่ออกจากเครื่องคุณเลย** — อินเทอร์เน็ตถูกใช้แค่ตอนกดดาวน์โหลดโมเดลเท่านั้น (ทดสอบได้: ตัดเน็ตแล้วทุกอย่างยังทำงาน)
- โปรแกรม**ปฏิเสธ**การพิมพ์ลงช่องรหัสผ่านโดยอัตโนมัติ
- โหมด Paste จะกู้คืน clipboard เดิมของคุณให้หลังพิมพ์เสร็จ

## 6. แก้ปัญหาที่พบบ่อย

| อาการ | วิธีแก้ |
|---|---|
| กดปุ่มลัดแล้วไม่มีอะไรเกิดขึ้น (macOS) | เช็ค Input Monitoring + Accessibility ใน System Settings → Privacy & Security แล้ว**ปิด-เปิดแอปใหม่** |
| ข้อความไม่ลงช่อง แต่ pill ขึ้น "Copied" | แอปปลายทางบล็อก paste — เปลี่ยน Insert method เป็น **Type** ใน Settings |
| ภาษาไทยถอดผิดเยอะ | Models → เปลี่ยนเป็น **Whisper Large-v3-Turbo** และตั้ง Language = ไทย (อย่าใช้ auto) |
| ช้าตอนพูดครั้งแรกหลังเปิดเครื่อง | ปกติ — โมเดลกำลังโหลดเข้า RAM ครั้งต่อไปจะเร็ว (ถ้า RAM น้อยให้ปิด low-memory mode ไม่ได้ช่วย ให้ใช้โมเดลเล็กลง) |
| เครื่อง RAM น้อย (<8GB) | ใช้ Whisper Base + ปิด AI cleanup หรือเปิด low-memory mode |

## 7. Build จาก source เอง (ทางเลือก)

ดูหัวข้อ **Building** ใน [README.md](../README.md) — สรุปคือ ติดตั้ง Rust + Node + CMake แล้ว `npm install && npm run tauri build` บนเครื่อง Windows/Mac ได้ตัวติดตั้งใน `src-tauri/target/release/bundle/`
