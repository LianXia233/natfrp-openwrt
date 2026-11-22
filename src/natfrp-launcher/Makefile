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

# 发布包使用的架构名
ifeq ($(ARCH),aarch64)
  NATFRP_BLOB_ARCH:=arm64
else ifeq ($(ARCH),arm)
  NATFRP_BLOB_ARCH:=armv7
else ifeq ($(ARCH),x86_64)
  NATFRP_BLOB_ARCH:=amd64
else ifeq ($(ARCH),i386)
  NATFRP_BLOB_ARCH:=386
else ifeq ($(ARCH),mipsel)
  NATFRP_BLOB_ARCH:=mipsle
else ifeq ($(ARCH),mips)
  NATFRP_BLOB_ARCH:=mips
else ifeq ($(ARCH),riscv64)
  NATFRP_BLOB_ARCH:=riscv64
else
  NATFRP_BLOB_ARCH:=
endif

define Package/natfrp-launcher/install
	$(INSTALL_DIR) $(1)/usr/bin
ifneq ($(NATFRP_BLOB_ARCH),)
	$(INSTALL_BIN) ./blobs/$(NATFRP_BLOB_ARCH)/natfrp-service $(1)/usr/bin/natfrp-service
	$(INSTALL_BIN) ./blobs/$(NATFRP_BLOB_ARCH)/natfrp-frpc $(1)/usr/bin/natfrp-frpc
else
	$(error 未支持的架构 "$(ARCH)"，请自行在 blobs/ 下放置二进制)
endif
endef

$(eval $(call BuildPackage,natfrp-launcher))