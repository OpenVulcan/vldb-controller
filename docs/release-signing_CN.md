# 控制器原生发行签名

`v0.2.4` 在保持既有 ABI 和协议的基础上修复内嵌 SQLite 的文件锁竞争，并接入 Windows 与 Linux 发行签名。

## 依赖来源

`Cargo.toml` 和 `Cargo.lock` 固定 SQLite `v0.1.7` 的完整提交 `f9803a98bc1bc294556862f9fc9d6ce2f913d40e`。GitHub 原生发行和 crates.io 发布是不同操作，本次通过固定 Git 提交重建控制器，不将尚未发布到 crates.io 的版本当成可下载的注册表依赖。其余外部依赖锁定版本保持不变。

## 签名顺序

1. 校验标签与工作区版本一致，构建对应原生平台的程序和 FFI 动态库。
2. 执行内嵌 SQLite 的真实锁竞争测试，要求明确返回占用错误，释放原持有者后可以重新打开。
3. Windows 从固定官方来源安装 SimplySign Desktop，通过组织机密生成动态口令；对程序和 FFI 动态库进行 Certum 签名、可信时间戳与固定身份验证。
4. 使用最终 Windows 签名字节打包并生成摘要。
5. Linux 对程序压缩包与 FFI 压缩包分别生成 OpenPGP 分离签名，再通过独立纯公钥密钥环验签。macOS 不进行代码签名或公证。
6. 上传全部资产为草稿。五个平台全部通过后，下载全部资产并复验摘要、Windows 签名和 Linux 签名，再公开发行。

## 凭据与公开信任

组织 Secrets 使用 `CERTUM_TOTP_EMAIL`、`CERTUM_TOTP_SECRET`、`GPG_PRIVATE_KEY`，私钥有口令时还需 `GPG_PASSPHRASE`。无需在仓库或开发机保存正式签名私钥。

`scripts/signing-policy.json` 固定 Certum 证书身份与 GPG 主公钥指纹；GPG 公开密钥保存在 `docs/release-gpg-public.asc`。证书续期与密钥轮换必须同步审核这些信任文件。SimplySign 自动登录沿用已经实测的客户端参数，不称作 Certum 官方无人值守 API。

同一 SimplySign 账户的其他仓库签名应按顺序执行；GitHub 并发组只约束单个仓库。本次公开资产应为十个压缩包、十份 SHA-256 文件以及四份 Linux `.asc` 文件，共二十四项。

## 工作区范围

本次从远端 `main` 建立隔离工作区执行。原本 `D:\projects\vldb-controller` 中尚未提交的协议、接口和客户端开发变更不属于本次发行内容。
