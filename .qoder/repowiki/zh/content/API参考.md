# API参考

<cite>
**本文档中引用的文件**
- [lib.rs](file://src/lib.rs)
- [Cargo.toml](file://Cargo.toml)
- [README.md](file://README.md)
</cite>

## 目录
1. [简介](#简介)
2. [模块结构与可见性规则](#模块结构与可见性规则)
3. [核心公开类型](#核心公开类型)
4. [BaseDeviceOps Trait 详细规格](#basedeviceops-trait-详细规格)
5. [EmulatedDeviceConfig 结构体详细规格](#emulateddeviceconfig-结构体详细规格)
6. [公开的Trait别名](#公开的trait别名)
7. [辅助函数](#辅助函数)

## 简介

`axdevice_base` 库为 ArceOS 虚拟机管理程序（Hypervisor）中的虚拟设备子系统提供基础的抽象。该库旨在 `no_std` 环境下运行，定义了所有模拟设备必须实现的核心接口和配置结构。

本API参考文档旨在成为开发者日常查询的权威手册，详细记录了库中所有公开暴露的接口、结构体、枚举和函数。文档内容严格按照源码顺序组织，为每个条目提供精确的签名、字段描述、方法行为解释及错误类型说明。

**Section sources**
- [lib.rs](file://src/lib.rs#L1-L83)
- [README.md](file://README.md#L1-L45)

## 模块结构与可见性规则

`axdevice_base` 库目前采用扁平化的单文件模块结构。所有公共API均直接在根模块（即 `lib.rs` 文件）中定义和导出，没有使用嵌套的子模块。

### 可见性规则
- 所有需要对外暴露的类型和函数均使用 `pub` 关键字声明。
- 使用 `pub use` 语句从依赖项（如 `axvmconfig`）重新导出 `EmuDeviceType` 枚举，确保其对库的使用者是可见的。
- 内部实现细节或测试代码（如 `#[cfg(test)] mod test;`）被明确标记为条件编译，不会暴露给外部用户。

这种设计简化了API的访问路径，开发者可以直接通过 `axdevice_base::TypeName` 的形式使用所有功能。

**Section sources**
- [lib.rs](file://src/lib.rs#L1-L83)

## 核心公开类型

本节列出并描述 `axdevice_base` 库中所有公开的顶级类型。

```mermaid
classDiagram
class EmulatedDeviceConfig {
+name : String
+base_ipa : usize
+length : usize
+irq_id : usize
+emu_type : usize
+cfg_list : Vec<usize>
+Default()
}
trait BaseDeviceOps~R~ {
<<trait>>
+emu_type() EmuDeviceType
+address_range() R
+handle_read(addr : R : : Addr, width : AccessWidth) AxResult<usize>
+handle_write(addr : R : : Addr, width : AccessWidth, val : usize) AxResult
}
trait BaseMmioDeviceOps {
<<trait alias>>
= BaseDeviceOps<GuestPhysAddrRange>
}
trait BaseSysRegDeviceOps {
<<trait alias>>
= BaseDeviceOps<SysRegAddrRange>
}
trait BasePortDeviceOps {
<<trait alias>>
= BaseDeviceOps<PortRange>
}
EmulatedDeviceConfig : "Configuration for device initialization"
BaseDeviceOps : "Core operations trait for all emulated devices"
BaseMmioDeviceOps : "Alias for MMIO devices"
BaseSysRegDeviceOps : "Alias for system register devices"
BasePortDeviceOps : "Alias for port I/O devices"
BaseMmioDeviceOps ..|> BaseDeviceOps : "aliases"
BaseSysRegDeviceOps ..|> BaseDeviceOps : "aliases"
BasePortDeviceOps ..|> BaseDeviceOps : "aliases"
```

**Diagram sources**
- [lib.rs](file://src/lib.rs#L32-L83)

**Section sources**
- [lib.rs](file://src/lib.rs#L32-L83)

## BaseDeviceOps Trait 详细规格

`BaseDeviceOps` 是所有模拟设备必须实现的核心特质（trait）。它定义了一个设备的基本操作集，包括查询其类型、地址范围以及处理读写请求。

该特质是一个泛型特质，其类型参数 `R` 必须满足 `DeviceAddrRange` 约束，这允许特质根据不同的寻址方式（如内存映射I/O、端口I/O等）进行适配。

### 方法规格

#### emu_type
- **签名**: `fn emu_type(&self) -> EmuDeviceType`
- **行为**: 返回此设备实例所代表的设备类型。返回值来自 `axvmconfig` crate 中的 `EmuDeviceType` 枚举。
- **副作用**: 无。
- **有效性要求**: 此方法应始终成功返回一个有效的 `EmuDeviceType` 值，不应失败。

#### address_range
- **签名**: `fn address_range(&self) -> R`
- **行为**: 返回一个 `R` 类型的实例，表示该设备占用的地址空间范围。这个范围用于设备发现和路由I/O请求。
- **副作用**: 无。
- **有效性要求**: 返回的地址范围必须是有效且一致的，通常在设备创建时初始化后不应改变。

#### handle_read
- **签名**: `fn handle_read(&self, addr: R::Addr, width: AccessWidth) -> AxResult<usize>`
- **行为**: 处理对设备的读取操作。`addr` 是相对于设备基地址的偏移量，`width` 指定了读取的数据宽度（如8位、16位、32位等）。
- **返回值**: 成功时返回 `AxResult<usize>`，其中 `usize` 包含从设备寄存器读取到的值。失败时返回一个 `AxError` 错误码。
- **可能抛出的错误**: 实现者可以根据具体情况返回各种 `AxError`，例如 `InvalidInput`（无效地址或宽度）、`NotSupported`（不支持的操作）等。
- **有效性要求**: `addr` 必须在 `address_range()` 返回的范围内，`width` 必须是设备支持的有效宽度。

#### handle_write
- **签名**: `fn handle_write(&self, addr: R::Addr, width: AccessWidth, val: usize) -> AxResult`
- **行为**: 处理对设备的写入操作。`addr` 是相对于设备基地址的偏移量，`width` 指定了写入的数据宽度，`val` 是要写入的值。
- **返回值**: 成功时返回 `Ok(())`，失败时返回一个 `AxError` 错误码。
- **可能抛出的错误**: 同 `handle_read`，可能因无效输入、不支持的操作或设备内部状态问题而失败。
- **副作用**: 此方法可能会改变设备的内部状态，例如更新控制寄存器、触发中断或启动数据传输。
- **有效性要求**: `addr` 和 `width` 的有效性要求同 `handle_read`。

**Section sources**
- [lib.rs](file://src/lib.rs#L48-L59)

## EmulatedDeviceConfig 结构体详细规格

`EmulatedDeviceConfig` 结构体用于在虚拟机创建时配置一个模拟设备的初始参数。

```rust
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct EmulatedDeviceConfig {
    pub name: String,
    pub base_ipa: usize,
    pub length: usize,
    pub irq_id: usize,
    pub emu_type: usize,
    pub cfg_list: Vec<usize>,
}
```

### 字段描述

- **`name`** (`String`): 设备的名称。这是一个必填项，用于标识和日志记录。
- **`base_ipa`** (`usize`): 设备的基中间物理地址（Intermediate Physical Address）。这是设备在客户机物理地址空间中的起始地址，为必填项。
- **`length`** (`usize`): 设备占用的地址空间长度（以字节为单位）。必须大于0，为必填项。
- **`irq_id`** (`usize`): 设备使用的中断请求（IRQ）ID。当设备需要向虚拟CPU发送中断时使用，为必填项。
- **`emu_type`** (`usize`): 设备类型的数值表示。虽然类型为 `usize`，但它应对应于 `EmuDeviceType` 枚举中的某个变体。此字段为必填项。
- **`cfg_list`** (`Vec<usize>`): 设备的配置列表，用于传递特定于设备的额外配置参数。此字段没有默认值，但可以为空向量 `Vec::new()`。

### 默认值
该结构体实现了 `Default` 特质。调用 `EmulatedDeviceConfig::default()` 将创建一个所有字段都为空或零值的实例：
- `name`: 空字符串 `String::new()`
- `base_ipa`: `0`
- `length`: `0`
- `irq_id`: `0`
- `emu_type`: `0`
- `cfg_list`: 空向量 `Vec::new()`

**注意**: 尽管提供了默认值，但在实际使用中，大多数字段都需要显式设置有效值才能正确初始化设备。

**Section sources**
- [lib.rs](file://src/lib.rs#L32-L46)

## 公开的Trait别名

为了方便使用，`axdevice_base` 定义了几个基于 `BaseDeviceOps` 的公开Trait别名，分别对应不同类型的设备。

### BaseMmioDeviceOps
- **展开形式**: `BaseDeviceOps<GuestPhysAddrRange>`
- **用途**: 该别名专门用于内存映射I/O（MMIO）设备。它将 `BaseDeviceOps` 的泛型参数 `R` 固定为 `GuestPhysAddrRange`，简化了针对此类设备的类型声明和边界。

### BaseSysRegDeviceOps
- **展开形式**: `BaseDeviceOps<SysRegAddrRange>`
- **用途**: 该别名用于模拟系统寄存器的设备。它将泛型参数 `R` 固定为 `SysRegAddrRange`。

### BasePortDeviceOps
- **展开形式**: `BaseDeviceOps<PortRange>`
- **用途**: 该别名用于传统的端口I/O（Port I/O）设备。它将泛型参数 `R` 固定为 `PortRange`。

这些Trait别名利用了Rust的 `#![feature(trait_alias)]` 功能，提高了代码的可读性和易用性。

**Section sources**
- [lib.rs](file://src/lib.rs#L73-L80)

## 辅助函数

### map_device_of_type
- **签名**: 
  ```rust
  pub fn map_device_of_type<T: BaseDeviceOps<R>, R: DeviceAddrRange, U, F: FnOnce(&T) -> U>(
      device: &Arc<dyn BaseDeviceOps<R>>,
      f: F,
  ) -> Option<U>
  ```
- **行为**: 该函数尝试将一个动态分发的设备引用（`Arc<dyn BaseDeviceOps<R>>`）向下转换为其具体类型 `T`。如果转换成功，则调用提供的闭包 `f` 并返回其结果；如果类型不匹配，则返回 `None`。
- **用途**: 在需要对特定类型的设备执行操作时非常有用，例如访问只有特定设备才有的专有方法。
- **示例**: 可以用来安全地访问一个自定义设备的私有配置或状态，前提是已知其具体类型。
- **稳定性**: 这是一个稳定且安全的API，利用了Rust的 `Any` 特质和动态类型转换机制。

**Section sources**
- [lib.rs](file://src/lib.rs#L61-L71)