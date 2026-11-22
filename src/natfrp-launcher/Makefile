#
# natfrp-launcher — SakuraFrp 守护进程二进制（架构相关）
#
# 这两个二进制只有预编译版本，作为 blob 处理：
#   blobs/<arch>/natfrp-service
#   blobs/<arch>/natfrp-frpc
#
# 取 blob 的方法（CI 与本地通用）:
#   curl -LO https://nya.globalslb.net/natfrp/client/launcher-openwrt/3.1.9/luci-app-natfrp_arm64.ipk
#   mkdir x && tar -xf luci-app-natfrp_arm64.ipk -C x
#   tar -xzf x/data.tar.gz -C x ./usr/bin
#   mkdir -p blobs/arm64 && cp x/usr/bin/natfrp-service x/usr/bin/natfrp-frpc blobs/arm64/
#
# 或者在设备上（apk-tools >= 3.0）:
#   apk extract /tmp/luci-app-natfrp.apk -o /tmp/x
#
include $(TOPDIR)/rules.mk

PKG_NAME:=natfrp-launcher
PKG_VERSION:=3.1.9
PKG_RELEASE:=1

PKG_LICENSE:=AGPL-3.0-only
PKG_MAINTAINER:=iDea Leaper

include $(INCLUDE_DIR)/package.mk

define Package/natfrp-launcher
  SECTION:=net
  CATEGORY:=Network
  TITLE:=SakuraFrp launcher daemon (prebuilt binaries)
  URL:=https://www.natfrp.com/
  DEPENDS:=
endef

define Package/natfrp-launcher/description
 SakuraFrp 内网穿透启动器核心进程 (natfrp-service) 与 frpc 客户端。
 预编译二进制，随包分发；对应守护进程不提供源码。
endef

# 发布包使用的架构名。
# 用 $(call) 在 install 展开时求值：解析阶段 $(ARCH) 可能还没定义。
# 参数允许带 ABI 后缀（如 aarch64_cortex-a53），按前缀匹配。
natfrp_blob_arch = $(strip \
	$(if $(filter aarch64%,$(1)),arm64,\
	$(if $(filter arm%,$(1)),armv7,\
	$(if $(filter x86_64%,$(1)),amd64,\
	$(if $(filter i386% i686%,$(1)),386,\
	$(if $(filter mipsel%,$(1)),mipsle,\
	$(if $(filter mips64el%,$(1)),mips64le,\
	$(if $(filter mips64%,$(1)),mips64,\
	$(if $(filter mips%,$(1)),mips,\
	$(if $(filter riscv64%,$(1)),riscv64,\
	$(if $(filter loongarch64%,$(1)),loong64,))))))))))))

define Build/Prepare
	mkdir -p $(PKG_BUILD_DIR)
endef

define Build/Compile
endef

define Package/natfrp-launcher/install
	$(INSTALL_DIR) $(1)/usr/bin
	$(if $(call natfrp_blob_arch,$(ARCH)),,$(error 未支持的架构 "$(ARCH)"，请在 blobs/ 下放置二进制))
	$(INSTALL_BIN) ./blobs/$(call natfrp_blob_arch,$(ARCH))/natfrp-service $(1)/usr/bin/natfrp-service
	$(INSTALL_BIN) ./blobs/$(call natfrp_blob_arch,$(ARCH))/natfrp-frpc $(1)/usr/bin/natfrp-frpc
endef

$(eval $(call BuildPackage,natfrp-launcher))