# 厂商组件与安装包依赖矩阵

日期：2026-11-06

本报告只做只读拆包、PE/IL 分析、PnP/服务查询和一次 DCHU 只读探测。没有运行五个安装包，没有安装、卸载、停止或删除任何驱动/服务，也没有修改 BIOS 或 EC。

## 结论

`DCHU` 不是 Windows 自带 API，也不是厂商用户态 DLL 的同义词。本机链路是：

```text
UEFI/ACPI 固件
  └─ ACPI\CLV0001 设备节点
       └─ AcpiBridge.sys（Insyde 编写、Microsoft WHCP 签名的 KMDF 驱动）
            └─ 自定义设备接口 {86994c74-ad43-4812-b7e7-0c420b5c5fd7}
                 ├─ InsydeDCHU.dll（厂商用户态包装层，可替换）
                 ├─ FnKey.exe / CC40.exe（厂商用户态应用，可替换）
                 └─ RAM CONTROL CENTER（直接 CreateFileW + DeviceIoControl）
```

因此：

- `InsydeDCHU.dll` 不是平台；它只是 `AcpiBridge.sys` 的用户态包装层。
- RAM CONTROL CENTER 已经自行实现该包装层对应的协议，不加载、不转发、不启动任何厂商用户态组件。
- `AcpiBridge.sys` 及其 PnP 设备是整个私有控制协议真正不可跳过的边界。
- 没有这个已签名内核驱动，普通用户态程序不能直接执行厂商的 ACPI `_DSM` 控制方法。
- 本机虽然存在 Microsoft 的 `ACPI\PNP0C14` WMI 设备，但厂商 `CLEVO_GET`/`CLEVO_EVENT` WMI 类没有注册成功，不能作为当前可用的驱动替代路径。

## RAM CONTROL CENTER 的实际依赖

### 必须保留

| 组件 | 本机证据 | 作用 | 删除后果 |
|---|---|---|---|
| `AcpiBridge.sys` 1.0.0.9 | `oem86.inf`；PnP `ACPI\CLV0001\1`；服务 `AcpiBridge`；签名有效 | 映射 DCHU 读、写、AppSettings、事件 IOCTL 到 ACPI/EC | 风扇、RGB、充电阈值、平台功耗模式、EC 温度等主控制路径全部失效 |

当前 FlexiKey 引擎使用 `WH_KEYBOARD_LL` 用户态钩子，不打开
`HKKbdFltr.sys`。因此 `HKKbdFltr.sys` 不是当前版本的运行时必需依赖；它只
保留为后续硬件级过滤/宏能力的有条件路径。

### 按功能保留

| 组件 | 作用 | Neo 当前关系 |
|---|---|---|
| `HKKbdFltr.sys` 2.1.0.6 | 硬件级键盘过滤/宏设备接口 `{2dc8164f-3a23-4d71-9fa6-91f9ead88613}` | 当前 FlexiKey 使用用户态低级键盘钩子，不打开该设备；仅保留协议/未来能力 |
| `AirplaneModeHid.sys` 1.5.0.9 | 把 `ACPI\CLV0003` 暴露成 Windows HID 无线电台控制器 | Neo 使用 `Windows.Devices.Radios`，不直接打开该驱动；删除后可能失去本机硬件飞行模式/无线电台控制入口 |
| `ANXUcmCxCD.sys` 1.9.2.20 | Analogix USB-C UCM 驱动 | 与 Neo 控制协议无关；属于 USB-C/PD/显示能力，不要因清理 CC 而误删 |

### 厂商用户态组件，Neo 不依赖

| 组件 | 实际用途 | Neo 是否加载/启动 |
|---|---|---|
| `InsydeDCHU.dll` | 包装 `CreateFileW`/`DeviceIoControl` | 否 |
| `HKFltrLib.dll` | 包装键盘过滤 IOCTL | 否 |
| `CC40.exe` | WPF 控制中心前端 | 否 |
| `FnKey.exe` | 厂商常驻热键、OSD、平台策略执行器 | 否 |
| `LaunchFnkey.exe` / `AddFn.exe` | 厂商启动链 | 否 |
| `DCHUService.exe` / `CCDCHUService` | 会话启动器、ACPI 通知监听、厂商进程拉起 | 否 |
| `HKClipSvc.exe` | 键盘过滤器的剪贴板共享服务 | 否 |
| `AcpiBridge1.sys` | `ACPI\CLV0002` 的附带组件，当前没有对应内核服务 | 否 |

### 本机不需要安装的附带组件

`HKMouFltr.sys` 是厂商鼠标过滤驱动。当前 Neo 的 FlexiKey 引擎使用用户态鼠标钩子，不打开该设备；它是否保留取决于是否还需要厂商原始鼠标宏功能，不是 Neo 主控制链的依赖。

`AcpiBridge1.sys` 虽然存在于驱动包和 PnP 节点中，但：

- `sc qc AcpiBridge1` 返回服务不存在（1060）；
- `DCHUService` 的反编译代码只打开 `ACPI#CLV0001#1`；
- Neo 只打开 `ACPI#CLV0001#1`；键盘过滤接口只存在于协议层，当前 FlexiKey UI 不打开它。

所以它不应当被误认为 Neo 的第二个必要驱动。

## 五个安装包逐项结论

| 安装包 | 大小 / SHA-256 前 16 位 | 实际内容与版本 | 本机状态 | 结论 |
|---|---|---|---|---|
| `Clevo_ControlCenter_7.057.zip` | 282,882,371 / `3f29bfc1a4307ee6` | Control Center 7.057；含 AcpiBridge、HKKbdFltr、HKClipSvc、FnKey/CC40、Intel PPM/DTT 等整套组件 | 驱动组件与本机已安装文件逐字节一致；FnKey 是 7.57，本机已是 7.101；CC40 是 7.56.1，本机已是 7.94.1 | 不安装。它只会降级厂商应用，不能提供 Neo 需要的新驱动 |
| `Clevo_SpeedShift.zip` | 370,101 / `999905448ab3765a` | Intel PPM 1.0.175，目标为 LNL/ARL，硬件 ID 为 `SWC\VID8086_*` | 本机是 AMD Ryzen 7 7745HX，无对应设备 | 不适用，不安装 |
| `Clevo_ANX.zip` | 74,798 / `3f8b1c3085436254` | Analogix `ANXUcmCxCD` 1.9.2.4 | 本机已安装 1.9.2.20，更新 | 不安装 |
| `Clevo_USB3.zip` | 75,842 / `f317fa2de7dd2086` | 同一 Analogix 驱动 1.9.0.0 | 本机已安装 1.9.2.20，更新 | 不安装 |
| `Clevo_Airplane.zip` | 44,060 / `52e6c42fd49f4fb5` | AirplaneModeHid 1.5.0.9 | 本机已安装同一文件，SHA-256 完全一致 | 已经安装，不重复安装 |

### 关键文件对照

Control Center 包内文件与本机已安装文件 SHA-256 相同的项：

```text
AcpiBridge.sys       5c01679117aab5629526dab211eb0d57b1035631cd498710d2450290dcf80f35
AcpiBridge1.sys      32e303abc610e28a20df3dd20f8cbbc5f78a1b24d8b1284a9ac0a175a3ef6e96
DCHUService.exe      7a8f670e35b72d9b270ca4abdb3d330c0451029ab107791006fc847a22eb2fc6
InsydeDCHU.dll       22fecadff27f4bf08cb4a17fe455ae490107b409e55827210f821947d65a9d47
HKKbdFltr.sys        a66bc6dace0f217cab5cfcf556897fbffbbeffacebca8eecef7749761ceaf6c6
HKFltrLib.dll        83b7a6874f0b0cc6f128f9fa59ab0a70c6942c6e6bb1d1a33f7090932899ec4d
HKClipSvc.exe        55305ee071099a4c9def10f906001ac3404e4ea18b43580213c65172347541cd
```

Airplane 包与本机 `AirplaneModeHid.sys` 的 SHA-256：

```text
29ec8bc50aada5ba726d9546735bccf6b7a86a92fc9d3c2766befa7ac802cef7
```

本机当前 Analogix 驱动：

```text
ANXUcmCxCD.sys       a34d86f1a5f0964aebdc9d73689b42cfb6b1922d602a45f05993d638b7eaf06f
```

包内 Analogix 驱动：

```text
Clevo_ANX.zip        299558bf87ac0b222e7225199de1e547b0057c9444390ebea77a04f7b32ece8f
Clevo_USB3.zip       24b73f2427364a6773872dffbdfe642916bc5f4d5e6422192263dd3ee778c7bf
```

## 为什么 RAM CONTROL CENTER 不是命令转发器

对部署的 `C:\ProgramData\NeoControl\neo-control.exe` 做 PE 和进程检查：

- 导入表没有 `InsydeDCHU.dll`、`HKFltrLib.dll`、`CC40.exe`、`FnKey.exe`；
- 运行中的 RAM CONTROL CENTER 模块列表没有加载任何厂商控制 DLL；
- 二进制直接包含设备路径和 IOCTL 常量；
- Rust 代码自行构造 `_DSM` 请求，解析 `AeoB` 描述符响应，自行编码 AppSettings 和键盘过滤帧；
- 不启动 `DCHUService`、`CCDCHUService`、`FnKey.exe` 或 `CC40.exe`。

只读实测：

```text
neo-acpi-probe.exe 12
command=12 returned=272
0000: 41 65 6F 42 ...
```

`AeoB` 响应和 272 字节有效载荷证明 Rust 进程可以直接访问 `AcpiBridge` 设备。

RAM CONTROL CENTER 自己唯一主动启动的服务是 `AcpiBridge`。`NeoControlBoot` 的服务依赖也是 `AcpiBridge`，这是为了在用户 shell 之前恢复平台功耗状态。

## 如果清理所有厂商用户态

保留 `AcpiBridge.sys` 以及必要的 PnP 驱动包时：

- Neo 的温度、风扇、RGB、充电阈值、平台功耗模式、GPU 切换等主控制仍可直接工作；
- Neo 不需要 `InsydeDCHU.dll` 或 `HKFltrLib.dll`；
- 不需要 `CC40.exe`、`FnKey.exe`、`DCHUService.exe` 或 `HKClipSvc.exe` 才能执行现有核心控制；
- 当前版 FlexiKey 的用户态钩子不要求 `HKKbdFltr.sys`；只有切换到硬件过滤路径时才需要它。

需要明确的功能缺口：

- `DCHUService` 会打开 CLV0001 的事件通道，并写入 `AppSettings(1, 12, 1)` 表示通知监听者存在。Neo 当前没有实现 `0x322404` 等待事件和 `0x322408` 读取通知值的循环。核心控制不依赖它，但如果要完整替代厂商 Fn 硬件事件/OSD 链路，需要补上这一层。
- `HKClipSvc` 是厂商键盘过滤器的剪贴板共享服务。Neo 当前不打开 `HKClipPipe`，也不使用它的共享内存，因此自己的用户态宏路径不依赖它。

## DCHUService 的安全与侵入性问题

反编译 `DCHUService.exe` 后确认：

- 它创建名为 `DCHUSERVICE` 的命名管道；
- 管道 ACL 给 `WorldSid` 读写权限；
- 字符串命令 `w_arg255`、`w_arg254`、`w_arg253`、`w_arg252` 进入高权限进程启动代码；
- 服务以 `LocalSystem` 运行；
- `OnStart` 会修改 `SYSTEM\CurrentControlSet\Control\ServicesPipeTimeout`，并删除 `DisableAutomaticRestartSignOn`。

这不是 Neo 控制协议的必要部分，却是明显的本地攻击面和系统副作用。是否移除应在保留驱动、确认 Fn/OSD 需求后做单独的可逆测试；不能直接删除驱动包。

## 驱动全部删除后还剩什么

只剩标准 Windows 和硬件厂商自己的通用驱动能力：

| 能力 | 无 AcpiBridge 时 |
|---|---|
| CPU/内存/磁盘/网络利用率 | 可读，PDH/CIM/性能计数器 |
| CPU 基准频率 | 可读，但 `Win32_Processor.CurrentClockSpeed` 在本机返回固定 3601 MHz，不能代表实时有效频率 |
| GPU 温度/利用率/显存 | 依赖 NVIDIA/AMD 通用驱动和 NVML/ADL 等，不由 CC 提供 |
| 电池电量、AC 状态 | 可使用 `GetSystemPowerStatus`、电池类驱动 |
| Windows 电源计划 | 可使用 `PowrProf`，但这不是 DCHU 平台功耗/EC 策略 |
| Wi-Fi/蓝牙开停 | 可走 `Windows.Devices.Radios`；本机无线 HID 仍依赖 AirplaneModeHid 设备 |
| 音量/亮度/刷新率 | 标准 Windows API/显示驱动，具体能力取决于硬件 |
| 普通用户态热键/宏 | 低层键盘钩子和 `SendInput` 可用 |
| 风扇模式/风扇曲线/RPM | 不可用 |
| EC/CPU/GPU 私有温度通道 | 不可用 |
| 键盘 RGB/灯效 | 不可用 |
| 充电阈值/FlexiCharger | 不可用 |
| 平台性能/Boost/功耗模式写入 | 不可用 |
| GPU MUX/独显直连切换 | 不可用 |
| 硬件级 Fn/OSD 事件与过滤 | 不可用或无入口 |

本机 `root\wmi` 中存在标准类 `MSAcpi_ThermalZoneTemperature`，但没有可用实例；`CLEVO_GET` 类也无法实例化。不能把 ACPI WMI 当成当前现成的备用控制通道。

## 工程边界

当前约束下，正确的复用边界是：

1. 继续使用 OEM 签名内核驱动作为固件/EC 边界；
2. 继续由 Rust 自己构造用户态协议和 UI；
3. 不再加载厂商 DLL，不启动厂商 UI/服务；
4. 不重写、替换或重新签名内核驱动；
5. 若未来要求“连 OEM 驱动也不保留”，则必须自写并签名 KMDF 驱动，或依赖固件另外暴露 ACPI WMI；这不是纯用户态 Rust 能解决的。

## 尚未执行的操作

- 没有安装五个包中的任何一个；
- 没有卸载 CC/FnKey/驱动；
- 没有停止 `CCDCHUService` 或 `HKClipSvc`；
- 没有删除 DriverStore、PnP 设备或注册表项；
- 没有做“仅移除厂商用户态、保留驱动”的整机可逆实验。

驱动检测和一键安装已经接入 Overview：只安装必需的 `AcpiBridge.inf`；`HKKbdFltr` 仅作为可选状态显示，不由默认安装按钮触发。驱动文件只放在本机应用旁边的 `drivers/`，不提交到公共仓库。

下一项安全实验应是：在保留 `AcpiBridge.sys` 的前提下临时停止厂商用户态服务与 AppX，再执行只读遥测和一次可回滚的电源/风扇写入，最后恢复服务。该实验必须单独进行，不与驱动清理混在一起。
