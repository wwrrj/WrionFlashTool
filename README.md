# WrionFlashTool

Wrion 系列的 Android 刷机工具项目。

## 仓库状态

本仓库目前尚未导入实现源码，也未提供可构建版本或发布产物。开发依赖、支持设备与使用说明将在源码导入后随实现维护。

## 项目分工

| 仓库 | 职责 |
| --- | --- |
| [WrionTools](https://github.com/wwrrj/WrionTools) | ROM 处理、移植方案执行与构建 |
| [WrionSchemes](https://github.com/wwrrj/WrionSchemes) | 系统方案、机型定义与插件资源 |
| [WrionPatcher](https://github.com/wwrrj/WrionPatcher) | APK / JAR 的静态 DEX 改写 |
| WrionFlashTool | 刷机工具的独立项目入口 |

ROM 构建与设备刷入是独立环节。具体支持的设备、包格式和操作范围，以后续实现及发布说明为准。
