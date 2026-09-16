# MCUpdate Client

MCUpdate Client 是由 Pink_Cats 维护的 Minecraft 自动更新客户端，fork 自 McPatch2 Rust Client。
客户端保持对 McPatch2 更新协议的兼容，并扩展了外部下载源、初始化清单、更新状态展示和可恢复错误提示。

感谢 McPatch2 原作者及贡献者提供的上游实现。本项目继续遵循仓库内的开源许可证。

## Crates 说明

| 名称                   | 用途                                                         |
| ---------------------- | ------------------------------------------------------------ |
| client         | 客户端主程序。用来执行文件更新过程，需要配置好后分发给玩家   |
| config_template_derive | 给客户端用的过程宏，用来根据源代码注释自动化生成客户端配置文件模板 |
| xtask                  | 用于ci/cd自动化打包的行为和命令                              |

## 常用命令

| 命令                                | 作用                                 |
| ----------------------------------- | ------------------------------------ |
| `cargo dev`                          | 开发场景下，启动客户端程序进行测试           |
| `cargo ci`                           | 自动构建场景下，打包客户端 |

## 更新器自更新

Windows 发布构建会在 `target/dist` 额外生成：

- `AutoUpdateClient-<版本>.exe`：可与当前运行中的更新器并存的新版本。
- `startlist.txt`：Loader 按从新到旧的顺序选择更新器。

发布连续版本时，通过 `MCUPDATE_PREVIOUS_STARTLIST` 指向当前发布源的
`startlist.txt`。构建器会把新 EXE 放在首行、保留历史回退项，并始终追加
旧版固定名 `AutoUpdateClient.exe`。服务端将新 EXE、`startlist.txt` 和新版
`Loader.jar` 作为普通文件更新即可；旧客户端完成下载后继续本次启动，下次
启动由 Loader 切换到新版。Loader 只有在新版正常退出后才删除旧 EXE；新版
缺失、损坏、无法启动或异常退出时保留旧版用于回退。

首个安全迁移版本还应设置 `MCUPDATE_LOADER_JAR`，指向已构建并验证的 Loader
产物；构建器会把它复制为 `target/dist/Loader.jar`，从而得到可直接放入
`.minecraft/autoupdate` 的三文件自更新集合。
