# WrionFlashTool 前端

基于 React、TypeScript 和 Vite 的桌面界面，负责刷机向导、设备选择、进度展示、日志与错误提示。设备和分区操作通过 Tauri 命令交给 Rust 后端执行。

## 开发

在仓库根目录执行：

```powershell
pnpm --dir frontend install --frozen-lockfile
pnpm --dir frontend dev
```

浏览器预览使用演示数据，适合调整页面和交互。真实设备操作需要通过 Tauri 桌面程序验证。

## 构建与检查

```powershell
pnpm --dir frontend build
pnpm --dir frontend lint
```

完整桌面构建使用仓库根目录的 `build.ps1`。依赖安装、桌面开发和后端测试见[项目首页](../README.md)。

新增页面或设备事件时，应同步核对前端类型、Tauri 命令参数和后端事件格式。
