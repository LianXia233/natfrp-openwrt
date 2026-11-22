#
# natfrpctl —— 页面状态与配置同步的后端（预编译二进制）
#
# Rust 二进制由 CI / 本地用官方工具链交叉编译产出，放在 files/natfrpctl-$(ARCH)，
# 这里只负责安装，不使用 OpenWrt 的 rust 包体系。
#
include $(TOPDIR)/rules.mk

PKG_NAME:=natfrpctl
PKG_VERSION:=1.0.0
PKG_RELEASE:=1

PKG_LICENSE:=MIT
PKG_BUILD_DIR:=$(BUILD_DIR)/$(PKG_NAME)-$(PKG_VERSION)

include $(INCLUDE_DIR)/package.mk

define Package/natfrpctl
  SECTION:=net
  CATEGORY:=Network
  TITLE:=SakuraFrp launcher status and configuration helper
  URL:=https://www.natfrp.com/
  DEPENDS:=@(aarch64||arm||x86_64||i386)
endef

define Package/natfrpctl/description
 把 uci 配置转换成守护进程运行配置，并向 LuCI 页面提供运行状态、WebUI 入口与明文入口。
endef

define Build/Prepare
	mkdir -p $(PKG_BUILD_DIR)
endef

define Build/Compile
endef

define Package/natfrpctl/install
	$(INSTALL_DIR) $(1)/usr/bin
	$(INSTALL_BIN) ./files/natfrpctl-$(ARCH) $(1)/usr/bin/natfrpctl
endef

$(eval $(call BuildPackage,natfrpctl))
