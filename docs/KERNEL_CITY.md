# Kernel City · Metropolis 应用建筑实验

在 [5c0/metropolis](https://github.com/5c0/metropolis) 上增量修改。继续使用 Rust / sysinfo、原有 20 FPS 模拟循环、Ratatui 城市、动画、主题与配置。本阶段是 Windows 终端城市，不是独立 GUI。

## 启动

从本 Fork 的 **Actions → Kernel City - Windows → 成功运行 → Artifacts** 下载 `KernelCity-Windows-x64`，解压整个文件夹，双击 `KernelCity.cmd`。推荐 Windows Terminal，窗口至少 100 × 30 字符。

开发运行：`cargo run --release -- --apps`。`metropolis.exe` 不带参数仍运行原版模式；原有 `--theme`、`--weather` 和配置继续生效。

**精确磁盘采集使用 Windows ETW，需要管理员权限。** 普通权限也能运行并显示 CPU、RAM、进程树和总 I/O；磁盘不可用时明确显示状态，不伪造磁盘数值或车辆。需要磁盘车辆时，自行打开管理员终端并运行 `metropolis.exe --apps`。程序不会自动提权，不结束其他进程，也不停止其他 ETW 会话。

## 看城市

| 真实状态 | 城市表现 |
|---|---|
| 一个应用及其后台进程 | 一栋带名称的前景建筑 |
| RAM 工作集之和 | 建筑宽度、高度，按对数缩放并平滑过渡 |
| CPU 占整机计算能力的比例 | 灯光密度、闪动和屋顶烟雾；超过 70% 亮红 |
| 磁盘读 / 写 | 车辆驶入 / 驶出该建筑楼门 |
| 应用启动 | 新建筑逐渐出现 |
| 整组进程退出 | 立即熄灯，约 3 秒后消失 |

Chrome、Steam、VS Code 分别保留前三个楼位，方便最小验证；其他应用按首次出现分配空位。楼位不随 CPU 排名变化。背景剪影、行人和星星沿用原有场景装饰。

- 鼠标左键点建筑，或 **Tab / 左右键**选择建筑。
- **PgUp / PgDn**切换街区；所有应用都能找到。
- **上下键**滚动 PID / 父 PID 明细，**Esc**关闭面板。
- **q / Ctrl+C**退出；原有 **r / s / d**继续切换雨、雪和调试信息。

## 指标与边界

- 每约 1 秒采样一次。进程 CPU 除以逻辑处理器数，展示整机口径；采样间隔按实际经过时间计算。
- Chrome、Steam、Code 的已知辅助程序按应用族聚合；其他同路径可执行程序按应用聚合，已知 helper / 同目录子进程沿父链归属。Shell、系统服务和 Steam 启动的游戏形成边界，避免把整台电脑误并为 Explorer 或 Steam。复杂第三方启动器尚未逐一适配。
- 用 PID + 创建时间隔离计数器；父进程退出后仍存活的 helper 保留原组。权限不足时只展示系统允许读取的数据。
- RAM 是各进程工作集之和，共享页可能重复计算，与任务管理器的私有内存口径不同。
- Windows sysinfo 的 `GetProcessIoCounters` 包括网络和设备 I/O，因此面板单独标为 **All I/O**。磁盘数据来自 ETW `DiskIo` 的 `TransferSize`，通过初始化与完成事件的 IRP 关联归属。缓存命中的读取不会成为物理磁盘车辆；系统延迟写回可能归到 System。
- ETW 有缓冲延迟。未能关联的事件与解码错误显示在状态行；缺失事件不猜测归属。数字表示本次采样期间收到的事件。程序只保存内存计数，不采集文件内容或网络地址。

## 实现位置

| 层 | 文件 |
|---|---|
| 进程采样、聚合、归一化 | `src/applications.rs` |
| Windows 磁盘采集 | `src/disk.rs` |
| 楼位、插值、生命周期与选择 | `src/city/applications.rs` |
| 现有模拟和渲染的接入点 | `src/main.rs`、`src/city/mod.rs`、`src/city/vehicles.rs` |

## 验证

`cargo test --all-targets` 检查三组应用聚合、PID 复用、孤儿进程、稳定 I/O、楼位、熄灯与 RAM/CPU 映射。Windows CI 另外运行真实系统采集和 ConPTY 终端交互。

`scripts/test_live_apps.py` 启动有真实 PID 的受控负载程序，并给测试副本命名为 chrome.exe、steam.exe、Code.exe 等。它检查三栋建筑、父子 PID、CPU、RAM、磁盘、退出。**这些是明确命名的测试夹具，不能等同于已验证真实 Chrome / Steam / VS Code 的所有行为。**

手动验收：先启动城市，再依次启动 Chrome、Steam、VS Code；播放视频 / 编译或打开较大项目，点击相应建筑核对数值；退出应用后观察其后台进程是否也退出。只有整组进程结束，建筑才消失。

`metropolis.exe --snapshot --samples 5` 可输出五次 TOML 快照，用于排查采样与归组，无需终端 UI。

原版 Windows 启动验证：[基线 CI](https://github.com/personality-chip/metropolis/actions/runs/34882807750)。

磁盘数据依据：[Microsoft DiskIo_TypeGroup1](https://learn.microsoft.com/en-us/windows/win32/etw/diskio-typegroup1)、[DiskIo_TypeGroup2](https://learn.microsoft.com/en-us/windows/win32/etw/diskio-typegroup2)、[ferrisetw](https://github.com/n4r1b/ferrisetw)。
