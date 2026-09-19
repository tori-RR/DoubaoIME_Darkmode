# DoubaoIME Darkmode

一个为 **Windows 豆包输入法** 制作的主题修改工具

这是独立的第三方小工具，不是官方插件，也不代表官方支持。

豆包输入法语音识别很好用，但 Windows 版目前只有白色主题，深色桌面上打字有点晃。于是做了这个皮肤助手。

> 主题通过读取已安装的输入法资源生成，不携带、也不分发官方皮肤文件

---

## 功能

### 主题
#### Dark
<png> dark.png xxx信号弱xxx <png>

#### Midlight
<png> midlight.png xxx信号弱xxx <png>

目前支持：

- 预设主题：Dark / Light / Midlight
- 自定义主题颜色和字体颜色
- 候选栏字体修改
- 候选栏透明度调整
- 自定义工具栏头像

### 工具栏

输入法关掉悬浮工具栏后，好像没有明显入口再打开。所以右键菜单里加了：

- 输入法设置
- 工具栏开关

---

## 使用方法

从 [Releases](https://github.com/tori-RR/DoubaoIME_Darkmode/releases) 下载：

```text
DoubaoIME_Darkmode.exe
```

请正常双击启动，不要「以管理员身份运行」。安装或卸载主题时，程序会单独请求管理员授权，并短暂重启豆包输入法。操作期间请尽量不要退出本工具。

启动后选择主题、字体、透明度或头像，再点安装。它会写入：

```text
C:\Program Files\DoubaoIME
```

需要恢复官方皮肤时，点卸载。卸载后会保留原件备份，方便以后校验和再装。如果提示需要恢复，或看到 `dmdm_backup_pending`、`dmdm_transaction`，请先保留这些目录，不要手动删。

Windows 10 若打不开，可能需要先安装 [WebView2 Runtime](https://developer.microsoft.com/microsoft-edge/webview2)。

---

## 文件位置

助手自己的数据在：

```text
C:\tmp\DoubaoIME Darkmode\
```

常见文件：

```text
DoubaoIME Darkmode.json
DoubaoIME Darkmode.json.bak
logo.png
webview\
```

`logo.png` 只在用了自定义头像后才有。`webview\` 是助手窗口的 WebView2 数据。

输入法皮肤备份在对应版本的：

```text
skin\default\dmdm_backup\
```

卸载后这份备份也还在。已验证版本会按内置哈希核验官方原件；未验证版本的备份，是安装前保存在本机的那份皮肤，不保证等于官方出厂文件。

---

## 原理

助手读取本机输入法 `skin/default` 里的 SVG / XML / PNG，按你选的颜色、字体、透明度和头像生成新皮肤，再写回对应目录。

主要改：

```text
候选栏背景
候选文字 / 选中颜色
候选栏字体
翻页按钮
悬浮工具栏背景
悬浮工具栏图标
工具栏头像
```

不改：

```text
ImeService.exe
ui.dll
tsf-oime.dll
```

---

## 兼容性

当前已验证：

```text
豆包输入法 v0.9.0.0 Windows
```

其他版本如果皮肤目录结构一致，仍可能允许尝试安装，并显示「未验证」。这只是实验兼容，不代表已经测过。

结构对不上，或主题生成预检失败时，会阻止安装。官方更新后请先看本页的兼容性说明。

---

## 注意事项

- 豆包输入法更新后可能覆盖主题，需要重新安装
- 官方皮肤结构变了，本工具可能暂时不能用
- 不要删除 `dmdm_backup`；卸载和恢复都靠它
- 中断安装留下的备份暂存或事务目录，请先保留

---

## 声明

本仓库不包含豆包输入法官方 SVG / XML / PNG 皮肤资源及其修改版本。

所有主题文件都在点击「安装」时，根据本机已安装的输入法资源即时生成。

本仓库以 MIT License 发布，详见 [LICENSE](LICENSE)。

---

## 编译

需要：

```text
Node.js
Rust（Windows MSVC 工具链）
Visual Studio Build Tools 的「使用 C++ 的桌面开发」
Tauri 2
WebView2
```

```powershell
npm ci
cargo fetch --manifest-path .\src-tauri\Cargo.toml --locked
npm run dev
```

正式打包要干净工作区，并且 Rust 依赖已经 fetch 过：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\pack_release.ps1
```

产物在 `dist\`，文件名带版本、`release` 或 `test`、提交号和时间戳，例如：

```text
DoubaoIME_Darkmode-1.0.0-release-<commit>-<时间>.exe
```

脚本默认还会复制一份到桌面，且不覆盖已有文件；CI 可用 `-NoDesktop`。GitHub Release 只挂 `DoubaoIME_Darkmode.exe`。
