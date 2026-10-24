<p align="center">
  <img src="assets/readme/hero.svg" width="100%" alt="Periph In One：Windows 外设电量工具，集中查看鼠标、键盘、耳机和手柄的电量">
</p>

<p align="center">
  <a href="https://github.com/FoLeaf/Periph-In-One/releases">发布</a> ·
  <a href="#支持的设备">设备兼容</a> ·
  <a href="#从源码构建">源码构建</a> ·
  <a href="contribution.md">参与贡献</a>
</p>

**Periph In One** 是面向 Windows 10/11 x64 的便携外设电量工具。把支持的 2.4G 接收器接到电脑，即可在一个面板里查看电量；收起窗口后，从系统托盘随时唤出。

Rust + Slint，使用软件渲染，无需 Python 或 WebView2 运行时。当前版本 **0.18.0**。

## 界面一览

以下图片由当前 Slint 界面直接渲染，使用明确标记的演示数据。百分比用于展示布局，不代表设备实测结果。

### 完整面板

窗口可缩放，设备卡片随宽度自动排列。保留上次读数与更新时间，区分当前读数和历史电量。

![完整面板：鼠标、手柄与键盘卡片，显示电量、连接状态和 VID/PID](assets/readme/overview.png)

### 托盘微缩面板

双击托盘图标查看紧凑列表，支持刷新、滚动和展开。

<p align="center"><img src="assets/readme/mini.png" width="280" alt="托盘微缩面板：设备名称、电量与更新时间"></p>

<details>
<summary>设备备注与设置</summary>

为设备设置备注，查看并复制硬件标识；设置中可切换开机启动。

![设备备注：硬件名称、接收器序列号、VID/PID 与备注输入框](assets/readme/remark.png)

![设置：开机启动开关](assets/readme/settings.png)

</details>

## 开始使用

1. 从 [Releases](https://github.com/FoLeaf/Periph-In-One/releases/latest) 下载 Windows x64 ZIP 并解压。
2. 在可写目录运行 `periph-in-one.exe`，连接支持的接收器。
3. 关闭窗口会收起到托盘。双击托盘图标打开微缩面板；完全退出使用托盘右键菜单中的“退出”。

- 启动时立即读取，之后每 60 秒刷新；也可手动刷新。
- 设备备注、历史电量与窗口尺寸保存在 EXE 同级的 `config/settings.json`。移动程序时一起移动 `config/`。
- 开机启动默认关闭，可在设置中开启。自启时直接进入托盘。
- 默认不联网、不遥测、不写日志；配置文件包含本机设备信息，请勿公开上传。

## 支持的设备

当前按接收器与协议匹配，**不代表支持同品牌的所有产品**。蓝牙和通用 HID 电量读取尚未实现。

| 设备 / 接收器 | 匹配标识 | 当前边界 |
| --- | --- | --- |
| ATK / VXE 鼠标 | `373B:1216`，Usage Page `FF05` / `FF02` | 已有该接收器的电量实测记录 |
| DARKRIM SHADOW 耳机 | `3654:6901`，`FF02:0002` | 已实现协议与解析测试，待补当前版本真机验收 |
| KZZI / Compx 键盘 | `3554:FA09`，`FF02:0002` | K75 身份识别已验证；该机固件未返回电量，显示 `—`；其他型号依固件而定 |
| BIGBIG WON / Zikway 手柄 | VID `413D`，`FF7A:0001`；已观察到 PID `2104` / `2106` | 烈风 GALE 已有电量实测记录；待机或未连接时可能不响应，唤醒后可重试 |

读取失败时保留历史电量并显示读数时间；没有历史值时显示 `—`。设备名称可能是接收器名称。接收器序列号不等于外设本体序列号；缺少可靠序列号时，换 USB 插口可能影响备注匹配。

## 从源码构建

需要 **Windows x64、Rust 1.92+**。MSVC 工具链需安装 Visual Studio Build Tools 的“使用 C++ 的桌面开发”和 Windows SDK；GNU 工具链需可用的 MinGW-w64 链接器、`windres` 和 `ar`。依赖版本以提交的 `Cargo.lock` 为准。

```powershell
git clone https://github.com/FoLeaf/Periph-In-One.git
cd Periph-In-One
cargo test --locked --all-targets
.\scripts\build-windows.ps1
.\target\release\periph-in-one.exe
```

创建便携 ZIP：

```powershell
.\scripts\package.ps1
```

生成 `dist/periph-in-one-0.18.0-windows-x64.zip`，包含程序、文档、图片、校验和与依赖许可，不包含本机配置。默认从 `target/release/` 取程序，可通过 `-Executable` 指定其他构建路径；已有同名 ZIP 时会拒绝覆盖。

GitHub Actions 在分支 push / PR 时执行格式、Windows MSVC 测试、Clippy、Release 构建和打包，产物保留 14 天。推送与 Cargo 版本一致的 `v*` 标签会复用完整检查，生成附带 ZIP 和 SHA256 的 Release 草稿，真机验收后手动公开。详见 [CI / CD 流程](.github/CICD.md)。Windows 10、多屏和不同 DPI 场景仍需要真机验证。

<details>
<summary>命令行诊断</summary>

```powershell
# 枚举当前支持的接收器标识，输出 JSON，不发送电量查询
.\target\release\periph-in-one.exe --list-devices

# 读取一次设备电量，不打开窗口
.\target\release\periph-in-one.exe --once
```

Release 程序使用 Windows GUI 子系统；在需要读取命令输出时通过管道获取，例如 `--list-devices | Out-String`。

</details>

## 开发与贡献

从 [contribution.md](contribution.md) 开始。欢迎提交设备适配、协议测试与界面改进；新设备请附脱敏后的 VID/PID、接口和可复现的读数证据。

```text
src/         设备读取、协议、状态、配置与 Windows 集成
ui/          Slint 页面与控件
assets/      应用图标和 README 图像
examples/    设备扫描与 README 截图生成
scripts/     PowerShell 构建与打包
licenses/    第三方许可
```

## 许可证与依赖

项目原创代码与文档采用 [MIT](LICENSE)。第三方组件保留各自的许可；Slint 按 [Royalty-free 2.0](licenses/Slint-Royalty-Free-2.0.md) 使用，分发时需遵守其署名条件。

[![Made with Slint](assets/slint-badge.svg)](https://slint.dev/)

设备访问使用 [hidapi](https://github.com/ruabmbua/hidapi-rs)，托盘使用 [tray-icon](https://github.com/tauri-apps/tray-icon)，Windows 集成使用 [windows-rs](https://github.com/microsoft/windows-rs)。
