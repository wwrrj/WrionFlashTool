# Harusora FlashTool 前端

这里是 Tauri 窗口中的 React/TypeScript 界面。页面负责向导流程、设备选择、刷入进度、日志查看和错误提示，刷机及设备操作由 `src-tauri` 中的 Rust 命令完成。

## 开发

在仓库根目录安装依赖并启动前端预览：

```powershell
pnpm --dir frontend install
pnpm --dir frontend dev
```

前端生产构建：

```powershell
pnpm --dir frontend build
```

完整桌面程序请使用仓库根目录的 `build.ps1`，它会先构建这里的前端，再编译 Tauri Release 程序。
