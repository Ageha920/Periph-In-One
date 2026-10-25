# 设备适配规范

本文是新增或扩展设备适配的统一方法论。[contribution.md](../contribution.md) 保留硬性红线，本文展开完整流程。三家厂商驱动的分析范例（ATK HUB、KZZI 258A 家族、BIGBIG WON）待复查核验后填入第 7 节。

## 1. 两层架构

| 层 | 位置 | 职责 |
| --- | --- | --- |
| 协议家族层 | `src/protocol.rs` | 帧构造、校验、解析的纯函数，无任何设备 I/O。一个家族一组 `xxx_request()` / `xxx_battery()` / `xxx_identity()` / `xxx_model_name()` |
| 厂商适配器层 | `src/vendor/<vendor>.rs` | `VendorAdapter` trait 实现：匹配规则、设备类别、能力声明、一次探测；每家一个文件 |
| 注册表 | `src/vendor/mod.rs` | `ADAPTERS` 常量数组，`scan()` 据此分发 |
| 传输与拓扑 | `src/devices.rs` | `open_pair` / `send` / `drain` / `KzziLink` 传输件，ContainerId 分组，`port_location`，扫描编排 |
| 身份 | `src/model.rs` | 四级身份链与历史合并，厂商无关，新适配不改此文件 |

现有家族：ATK FF05/FF02、ATK FF04（V2 鼠标，含 CompX 3554 段）、DARKRIM TLV、KZZI/Compx 0x13、BIGBIG WON(Zikway) FF7A A5。

规则：

- 适配器只组合 `protocol.rs` 的帧函数，不内联构造字节；probe 与主应用共用同一份构造器，避免协议代码分叉。
- 传输怪癖（如 KZZI 键盘必须走 `KzziLink` 原始句柄的 control endpoint 写入）留在适配器 `read` 内部，trait 不假设统一传输。
- 错误以 UI 消息形式从 `Outcome.message` 浮出，适配器不向上抛传输错误。

## 2. 公模（OEM）规则

1. 同一协议家族、不同品牌：共用家族纯函数，每品牌一个薄适配器，各自 VID 与型号表；绝不把 A 品牌的 VID 集扩大到 B 品牌。
2. 同一品牌、不同协议（如 ATK 同时使用 373B、CompX 3554、Zikway 413D 方案）：各自家族、各自适配器。
3. 身份链以 VID 为键的一部分（`uid_key(vid, uid)`、`serial_key(vid, pid, serial)`），公模换牌后 VID 不同自然形成独立记录，无需特判。

## 3. 匹配与收录

- 匹配必须锚定 VID 加 Usage Page / Usage；多家共用的通用 VID（如键盘常见的 0x258A）绝不能只按 VID 匹配。
- PID 仅在家族跨固件人格保持同一 vendor interface 时才放开（例：BIGBIG WON 2104/2106 双固件态共享 FF7A）。
- 档位沿用 contribution.md 的三档：已识别（匹配规则成立）、已实现（协议编码完成）、已实测（真机验证）。未接触过真机的协议结论最高标已实现，不得声称已实测。
- matcher 收录：无线类设备可凭静态分析结论进入（标已实现未实测）；有线型号、显示器、dongle 不进 matcher，只记入附录设备表。通用 VID 的新条目必须附 Usage Page 锚定证据。

## 4. 能力声明

适配器通过 `capabilities()` 声明能力集（`src/vendor/mod.rs` 的 `Capabilities`）：

- `BATTERY`：电量读取，当前唯一已实现能力。
- `LIGHTING`、`CONFIGURATION`：预留位。灯效、DPI、改键等写命令的帧格式记录在 probe 研究注释中；写路径落地前，任何适配器不得声明这些能力。

能力按产品线声明；线内个别型号不提供电量（如 KZZI K75）由适配器按型号甄别后经 `Outcome.no_battery` 上报，UI 据此区分在线不提供电量与未响应。

## 5. 适配流程

1. **研究先行**：写 `examples/probe_<vendor>.rs`（模板：`examples/probe_bigbigwon.rs`；多厂商命令空间扫描参照 `probe_identity.rs`）。参数化帧构造器先进 `protocol.rs`，probe 从库导入，不携带副本。结论写在 probe 顶部 doc comment，注明来源与许可。
2. **协议层**：在 `protocol.rs` 为家族添加纯函数，帧必须带校验（和校验或 CRC，按协议实际）。
3. **适配器**：新建 `src/vendor/<vendor>.rs` 实现 `VendorAdapter`，注册进 `ADAPTERS`。
4. **身份**：协议有每台唯一身份命令的，在 `read` 中读出并填 `Outcome.uid`，身份链自动接入。
5. **测试**：`tests/core.rs` 添加帧构造、坏帧、截断、校验和、电量边界测试，用真机捕获字节；核实 `0%` 是否有效，未知电量不得转成零。
6. **文档**：README 设备表加行；`docs/VALIDATION.md` 按版本记录验收（帧格式、真机读数、边界、测试数、SHA-256）。
7. **检查**：`cargo fmt --all -- --check && cargo test --locked --all-targets && cargo clippy --locked --all-targets -- -D warnings`，再真机跑 `--once` 与 `--list-devices`。

## 6. 身份链接入

优先级从强到弱：协议 uid（`Outcome.uid`）> USB 序列号（`devices::raw_serial_number` 直读）> USB 父口位置（`port_location`，跨重枚举稳定）> 拓扑兜底。适配器只需要做一件事：协议能报告每台身份就填 uid，其余由 `model.rs` 完成。

## 7. 范例

三节均已随复查落位（2026-10-25）：7.1 五条 PASS 加一条表格污染清洗后采用，7.2 五条 PASS 一条修正后采用，7.3 七条全 PASS。

### 7.1 ATK HUB（跨家族品牌）

来源：ATK HUB v3.2.30 Electron 应用两个 asar 包静态分析（app.asar 5956965c…347a、main/app.asar 1e4b689f…b56b），复查五条 PASS、设备表一条确认污染并清洗后采用。

**家族版图**：ATK 自有 VID 0x373B 按 Usage Page 分家族——FF05:0001 鼠标/接收器线（89 个 PID；同页还有显示器、数位板、掌机、眼镜、音箱、摄像头、坞站、麦克风外设线，明确不收录）、FF04:0002 V2 鼠标线（179 个 PID）、FF60/FF10 BYKB 键盘、keyLinker（413D 的 AXEPro 同走此通道）、FFB2 Nearlink、0x0C 耳机 MB 协议等。另有 CompX VID 0x3554 的 Dragonfly 线（FF04:0002，12 个 PID）：与 KZZI 的 0x13 接收器协议同 VID 不同家族——共享 VID 不等于共享协议，识别必须锚定 Usage Page 与帧格式，这是第 2 节公模规则的活例子。

**FF05 鼠标帧**（1216 已实测）：63 字节输出报告、reportId 8；[0] = sum(bytes[1..62]) & 0xFF、[1] = cmdCode 0x72、[5] = commandId（应答匹配键）；GetBatteryLevel=7 应答 data[0] 为电量，GetBatteryChargingStatus=15 应答 1 为充电中。与主应用 atk_sub_request / atk_battery 逐字段一致。

**FF04 V2 鼠标帧**（已实现未实测）：16 字节帧走 feature report（reportId 8），[0] = commandId、[1] = commandStatus、[2..3] = EEPROM 地址大端、[4] = dataValidLen、[5..14] = 数据、[15] = 校验 = 0x55 - ((0x08 + sum(bytes[0..14])) & 0xFF)；GetBatteryLevel=4，应答 data[0] = 电量、data[1] = 充电状态、data[2..3] = 电压大端。实现在 protocol.rs 的 atk_ff04_* 与 vendor/atk_v2.rs。0x3554 的 FF02 早期批次段（0xF31F、0xF400-0xF410）同协议只是推断，无接口证据，不收录。

**每台身份（记录档，未实现）**：BYKB 键盘 UUID 命令 encode(130,1,0×6)，应答 8 字节按大端折叠为 48 位（0x12/0x14 前缀为三模机型）；FF04 GetMouseCIDMID=16（CID/MID 加 uniqueId，设备表以预期 CID,MID 做型号一致性校验）；FF05 MouseCidMid=74 与 GetChipId=71；keyLinker 设备探针（应答字节 19/20 拼 0x2025）。

**写命令（记录档，LIGHTING/CONFIGURATION 预留位）**：FF05 SetLEDLightEffect=20、接收器 133/134；FF04 Set4KRGBMode=20、SetDongleLightMode=24、EEPROM 档位颜色地址 44..77（成对 CRC 槽）；BYKB 区域灯色 encode(4, 区, [参数,R,G,B]) 与矩阵 get/write_custom_led_matrix=10/11；耳机 EQ / 麦克风 / 环绕域。

**复查教训（表污染）**：原报告 FF05 鼠标/接收器表（122 行）混入 31 个外设 PID 与 1 个 OTA 裸 dataFilter 对象——成表脚本只排除了 monitor/tablet/handheld 三类标记。清洗改以对象自带的专属 ID 字段（monitorID / tabletID / handheldID / aiGlassesID / speakerID / webcamID / dockID / microphoneID）与 meta.deviceType 为准，而非仅 firmwareMark。这正是第 3 节档位纪律的价值：进 matcher 前逐条核对，清洗后 89 个 PID 全部有独立提取证据。

### 7.2 KZZI 258A 家族（Beiying OEM 双传输线）

来源：KZZI RGB Keyboard 驱动 DeviceDriver.exe 静态分析（SHA-256 2a35647a…77079），复查六条载荷性结论五条 PASS、一条修正后采用。

**家族关系**：258A 与 3554 是同一 OEM（Beiying）同一驱动引擎的两条传输线。键盘本体 USB 直连时以 258A:<PID> 枚举，走 0x208 字节 feature report 有线协议：reportId 0x09，命令族 0x82（AccessData 单帧）/ 0x84 / 0x88（分页写）/ 0x87（分页读，GetPower 用它读 2 字节），大块数据按 65 字节 0x0A 分块帧承载（62 字节负载，逐包 0xFF-求和校验）。2.4G/蓝牙接收器以 3554:FA09 或伪装 Apple VID 的 05AC:024F 枚举，走 20 字节 reportId 0x13 协议——即主应用已支持的家族。同一型号两种形态：K75RGB Lite 有线是 258A:01B0，经接收器是 3554:FA09。

**识别锚点**：接收器 = FF02/0002 + 20 字节 Input/Output 报表 + 报表 ID 0x13（厂商五重 caps 校验），通用 VID 0x258A 绝不能只按 VID 匹配；有线 = VID/PID（Cfg.ini 表内）+ FF00/01 或 FF02/01 或 FF02/02 + feature 报表长度 0x208，报表 ID 从 caps 派生。型号身份 = 新代档案的 Psd 六字节键（GetPSD cmd 0x05 应答与 KB.ini Psd= 同源），与主应用 `kzzi_model_name` 表交叉验证一致（注意 KB.ini 为十六进制文本、表内为十进制）。01C8/01DA/024D 三个档案缺 Psd，为异常档案。

**电量（复查修正后口径）**：接收器路径的 0x4A 命令在本驱动中有包装函数（0x40E560：发 20 字节帧、回读两字节，与 `kzzi_battery` 的 (level, state) 形态一致；静态可达性未证，虚调用槽 +0xA0 的 8 处 UI 区调用点未能绑定设备对象）。主应用的 0x4A 实现来自真机探测，与此静态锚点互相印证。有线路径的电量（0x87 双页读）未实测。原报告的本驱动无 0x4A、电量仅有线路径两条结论被复查推翻或降档。

**在线/未响应口径（真机）**：接收器应答 0x05 身份即视为在线。0x4A 无应答有两种含义——固件不提供电量的型号（K75 RGB key，Psd `[2,0,0,0,0,55]`，真机验证）经 `protocol::kzzi_battery_private` 标记为主应用显示已连接 · 该型号不提供电量；其余型号无应答仍按未响应处理（可能键盘关机），待真机逐一验证后再加入标记。

**收录决策**：56 个 258A 型号全部是有线本体 PID，接收器形态已被 3554:FA09 适配覆盖；按第 3 节规则有线型号不进 matcher，全表进附录 A，接收器识别的最终锚点沿用 caps 事实而非 VID。

### 7.3 BIGBIG WON（Zikway FF7A 家族）

来源：BigBigWonAssistant 1.0.7.7 的 DevMgr.dll 静态分析（SHA-256 41cf4c1e…8c436）。

**身份模型**：全部型号共享同一组 VID/PID——413D×{2104 连接态, 2106 底座态, 2114}，另有 Switch Pro（057E:2009）与 DS4/DS3（054C:05C5、054C:0DF2）模拟人格；型号不按 PID 区分，按 USB 产品字符串区分（ZJ-GALE→GALE、C4_SERVER→Gale2、ZJ-C1ZJ→Rainbow3、ZJ-MJC6→Aether、ZJ-BLACK_→Mage、ZJ-C2SL→BLITZ2 等 12 类映射）。厂商客户端自身不过滤 FF7A（DevMgr.dll 全文零命中，FF7A/usage1 是固件描述符的属性），主应用按 VID 413D + FF7A/usage1 匹配且不锁 PID，比厂商更严且与实测一致。

**电量**：唯一电量操作码 04，应答处理器要求长度字节恰好 6（A5 06 04 电量 状态 校验和）；`protocol.rs` 的 `bigbigwon_battery` 已按此对齐（长度不为 6 即拒绝）。

**身份链第一级**：GetUUID（EF）请求 A5 0C EF 加 8 字节零选择器，应答帧 [3..10] 为 8 字节 UUID；静态语义与现有实现逐字段一致。

**命令空间（记录档，未实现）**：短查询约 28 个操作码（04/0B/D3/D4/D5/D6/D9/DA/DB/DC/DD/E1/E2/F5/F6/F7/FA/70/72/73/F8 等），设置类约 25 个（02 SetLogoLED、05 Switch2UDisk、0E Save2Device、1A Reset、72 亮度、73 灯效模式、D2/D4/DA/DC/E1/F6/F7/FB/FD 等），A4 长帧配置族按 240/280/335/454/484/508 字节分 7 个版本；查询用 len=04、设置用 len=05 且多一个数据字节是普遍规律。新增的 GetWirelessFlag（FA，请求 A5 04 FA A3，应答 A5 05 FA flag 校验和）已加入 examples/probe_bigbigwon.rs，待连接态真机验证。

**灯效（LIGHTING 预留位）**：F5 GetLogoLED、02 SetLogoLED（A5 0D 02 加 10 参数字节）、72 亮度读写、73 灯效模式读写、70 SetLEDShow 提交、DD 查询与 51 设置充电灯效；SetLED 有 16 字节定长与变长（头部 12 字节加 N×3 RGB 三元组）两个重载，长度字节随区数递增。JSON 字段到参数的映射在 WndMgr.dll，未展开。

**复查锚点**：帧常量 0xad0404a5（04）、0xa3fa04a5（FA）、0x7cd304a5（D3）、0x197004a5（70）均为厂商预计算常量，与 8 位环绕累加互证；应答分发三层结构（入口 0x10024113、索引表 0x10024EB8、跳转表 0x10024E64）。

### 7.6 泥炭 SOUNDPEATS（蓝牙音频，BTHENUM 系统路径）

来源：Windows 蓝牙栈行为验证（BTHENUM 枚举 + DEVPKEY 属性），SOUNDPEATS Space 真机实测；无需厂商驱动——经典蓝牙音频设备的电量由 Windows 经 HFP/AVRCP 收取，落在免提服务节点上。

**识别与身份**：枚举 `BTHENUM` 总线（非蓝牙设备类，免提节点与设备节点分属不同类），以实例 ID 中的 12 位十六进制 MAC 为物理标识（`bt:<MAC>`）；设备节点与免提服务节点按 MAC 合并，仅列主类为音频/视频（class of device 位 8..13 = 4）且系统连接标志为真的设备。电量取自免提节点属性 `{104EA319-…}`/2 单字节，未知时不显示——连接状态与电量读数彼此独立。

**边界**：只覆盖 Windows 已记录电量的经典音频设备；蓝牙 HID 外设（LE 触控板等）不走此路径。蓝牙设备无 HID 序列号，历史合并靠 MAC，跨适配器重配对不影响。

## 附录 A：KZZI 258A 有线型号表（已识别档）

来源：驱动 Cfg.ini 明文全表，Psd 映射经复查抽查验证。全部为有线本体 PID，接收器形态以 3554:FA09 / 05AC:024F 枚举（已被现有适配覆盖），按第 3 节规则不进 matcher。代际：旧代无 Psd，新代（00CF 起）带 Psd 档案；标注沿用厂商 DevName 原文。

| PID | 型号 | 代 | 标注 |
| --- | --- | --- | --- |
| 0049 | KZZI-G68 Keyboard | 旧 | |
| 004A | KZZI-61RGB Wired Keyboard | 旧 | Wired |
| 004B | KZZI-100RGB Wireless Keyboard | 旧 | Wireless |
| 004C | KZZI-87RGB Keyboard | 旧 | |
| 004D | KZZI-71RGB Wired Keyboard | 旧 | Wired |
| 004F | KZZI-G87RGB Wired Keyboard | 旧 | Wired |
| 0055 | KZZI-61 Keyboard | 旧 | |
| 0056 | KZZI-100RGB Keyboard | 旧 | |
| 0059 | KZZI-84RGB Keyboard | 旧 | |
| 005A | KZZI-71RGB Keyboard | 旧 | |
| 005B | KZZI-987RGB | 旧 | |
| 005C | KZZI-G68RGB-UK Keyboard | 旧 | |
| 005D | KZZI-857 Keyboard | 旧 | |
| 005E | KZZI68RGB Bluetooth Dual Mode | 旧 | 蓝牙双模 |
| 0064 | KZZI-68RGB Wired Keyboard | 旧 | Wired |
| 0065 | KZZI-61RGB ISO Return Keyboard | 旧 | |
| 0067 | KZZI-100PRO Keyboard | 旧 | |
| 0068 | KZZI-987RGB Wired Keyboard | 旧 | Wired |
| 0069 | KZZI-71RGB Wireless | 旧 | Wireless |
| 006F | KZZI-68Plus Keyboard | 旧 | |
| 0070 | KZZI-K980 RGB Keyboard | 旧 | |
| 0075 | KZZI-G68RGB ISO Return Keyboard | 旧 | |
| 0078 | KZZI-87 Keyboard | 旧 | |
| 0079 | KZZI-68Pro Keyboard | 旧 | |
| 007A | KZZI-61 RGB Bluetooth Dual Mode | 旧 | 蓝牙双模 |
| 007C | KZZI-87T RGBWired Keyboard | 旧 | Wired |
| 007D | KZZI-G87RGB Wired Keyboard | 旧 | Wired |
| 0089 | KZZI-96 RGB Keyboard | 旧 | |
| 008C | KZZI-61RGB ISO ReturnWired | 旧 | Wired |
| 00A2 | KZZI-68RGB Wired Keyboard | 旧 | Wired |
| 00AA | KZZI-100RGB Keyboard | 旧 | |
| 00C6 | KZZI-K61 RGB Keyboard | 旧 | |
| 00CF | Kzzi-K75 RGB | 新 | |
| 0108 | Kzzi-K68 | 新 | |
| 0112 | Kzzi-K75 RGB key | 新 | |
| 0114 | Kzzi-K84 | 新 | |
| 0149 | Kzzi-K75Pro | 新 | |
| 0160 | Kzzi-Z98 | 新 | |
| 017D | Kzzi-K20RGB | 新 | |
| 0186 | Kzzi-K68Pro | 新 | |
| 0195 | Kzzi-K98 | 新 | |
| 01AA | Kzzi-K75Pro | 新 | |
| 01AE | Kzzi-K87 | 新 | |
| 01B0 | KZZI-K75RGB Lite | 新 | |
| 01B2 | Kzzi-K21 | 新（无 Fw 键） | |
| 01B6 | Kzzi-i75 RGB | 新 | |
| 01C8 | Kzzi-K98Pro | 异常（缺 Psd） | |
| 01D5 | Kzzi-K87MAX | 新 | |
| 01DA | Kzzi-K87Pro | 异常（缺 Psd） | |
| 024D | Kzzi-K75V2Pro | 异常（缺 Psd） | |
| 0253 | Kzzi-K75V2 | 新 | |
| 0260 | Kzzi-Z75 | 新 | |
| 027D | Kzzi K98V2 | 新 | |
| 029B | Kzzi-i98 | 新 | |
| 029F | Kzzi-i20 | 新 | |
| 02E2 | Kzzi-i98 | 新 | |
