# Security policy · นโยบายความปลอดภัย

## Reporting a problem

Please report security problems **privately**, not in a public issue:
open the repository's **Security** tab › **Report a vulnerability**
([Security tab](https://github.com/ball46/duet/security)).

Say what you found, how to reproduce it, and what someone could do with it. I aim to answer
within 7 days. Once a fix is released, the advisory is published with credit to you, unless you
would rather stay anonymous.

Examples of what counts: a file or folder that makes duet write, delete or overwrite something it
should not (for example through links or junctions), a sync or copy that escapes its folders, a
report that leaks a value it promised to mask, or any way to make duet use the network.

Only the latest release gets security fixes.

## การแจ้งปัญหา

กรุณาแจ้งปัญหาด้านความปลอดภัย **แบบส่วนตัว** อย่าเปิดเป็น issue สาธารณะ
โดยเข้าแท็บ **Security** ของ repository แล้วเลือก **Report a vulnerability**
([แท็บ Security](https://github.com/ball46/duet/security))

บอกว่าเจออะไร ทำซ้ำได้อย่างไร และคนอื่นอาจใช้ช่องโหว่นี้ทำอะไรได้ ผู้ดูแลตั้งใจจะตอบภายใน 7 วัน
เมื่อปล่อยรุ่นที่แก้แล้ว จะเผยแพร่รายละเอียดพร้อมให้เครดิตผู้แจ้ง เว้นแต่ผู้แจ้งต้องการไม่เปิดเผยชื่อ

ตัวอย่างปัญหาที่ควรแจ้ง เช่น ไฟล์หรือโฟลเดอร์ที่ทำให้ duet เขียน ลบ หรือเขียนทับสิ่งที่ไม่ควรแตะ
(เช่นผ่าน link หรือ junction) การซิงก์หรือคัดลอกที่หลุดออกนอกโฟลเดอร์ที่เลือก รายงานที่เผยค่าที่ควรถูกปิดไว้
หรือวิธีใดก็ตามที่ทำให้ duet ใช้เครือข่ายได้

แก้ไขด้านความปลอดภัยเฉพาะรุ่นล่าสุดเท่านั้น
