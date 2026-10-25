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

现有家族：ATK FF05/FF02、DARKRIM TLV、KZZI/Compx 0x13、BIGBIG WON(Zikway) FF7A A5。

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

## 7. 范例（待填）

以下三节在独立会话完成的驱动分析报告经复查核验后填入，作为本规范的 worked example：

- 7.1 ATK HUB（Electron/明文 JS 设备表，跨 373B / CompX / Zikway 家族）
- 7.2 KZZI 258A 家族（与已支持的 3554 CompX 家族的关系、56 型号表、电量协议）
- 7.3 BIGBIG WON（DevMgr.dll 设备表、FF7A 命令空间完整定义）
