#
# luci-app-natfrp —— SakuraFrp 内网穿透（单包）
#
# 一个包里包含：守护进程二进制、Rust 后端 natfrpctl、LuCI 页面与运行脚本。
# 目标是 OpenWrt 25.12+（apk 包管理），默认针对 aarch64_cortex-a53 编译。
#
# 编译输入（由 CI / scripts/build.sh 生成，不入库）：
#   files/natfrpctl-$(ARCH)          官方 Rust 工具链交叉编译产物
#   files/blobs/<blob-arch>/*        守护进程与 frpc 预编译二进制
#
include $(TOPDIR)/rules.mk

PKG_NAME:=luci-app-natfrp
PKG_VERSION:=3.2.0
PKG_RELEASE:=1

PKG_LICENSE:=MIT
PKG_BUILD_DIR:=$(BUILD_DIR)/$(PKG_NAME)-$(PKG_VERSION)

include $(INCLUDE_DIR)/package.mk

# 发布包使用的架构名。在 install 展开时求值：解析阶段 $(ARCH) 可能还没定义。
NATFRP_ARCH_MAP := aarch64:arm64 arm:armv7 x86_64:amd64 i386:386 i686:386 \
	mipsel:mipsle mips64el:mips64le mips64:mips64 mips:mips riscv64:riscv64 \
	loongarch64:loong64

natfrp_blob_arch = $(firstword $(foreach e,$(NATFRP_ARCH_MAP),\
	$(if $(findstring $(word 1,$(subst :, ,$(e))),$(1)),$(word 2,$(subst :, ,$(e))))))

define Package/luci-app-natfrp
  SECTION:=net
  CATEGORY:=Network
  TITLE:=SakuraFrp launcher with LuCI page
  URL:=https://www.natfrp.com/
  DEPENDS:=@(aarch64||arm||x86_64||i386)
endef

define Package/luci-app-natfrp/description
 SakuraFrp 内网穿透：守护进程、Rust 状态后端与 LuCI 页面（状态展示、配置同步、
 WebUI 与远程管理入口、可选明文入口）。
endef

define Package/luci-app-natfrp/conffiles
/etc/config/natfrp
endef

define Build/Prepare
	mkdir -p $(PKG_BUILD_DIR)
endef

define Build/Compile
endef

define Package/luci-app-natfrp/install
	# 守护进程与后端
	$(INSTALL_DIR) $(1)/usr/bin
	$(if $(call natfrp_blob_arch,$(ARCH)),,$(error 未支持的架构 "$(ARCH)"，请在 files/blobs/ 下放置二进制))
	$(INSTALL_BIN) ./files/blobs/$(call natfrp_blob_arch,$(ARCH))/natfrp-service $(1)/usr/bin/natfrp-service
	$(INSTALL_BIN) ./files/blobs/$(call natfrp_blob_arch,$(ARCH))/natfrp-frpc $(1)/usr/bin/natfrp-frpc
	$(INSTALL_BIN) ./files/natfrpctl-$(ARCH) $(1)/usr/bin/natfrpctl

	# LuCI 页面
	$(INSTALL_DIR) $(1)/www/luci-static/resources/view/natfrp
	$(INSTALL_DATA) ./htdocs/luci-static/resources/view/natfrp/main.js \
		$(1)/www/luci-static/resources/view/natfrp/main.js

	# 菜单与权限
	$(INSTALL_DIR) $(1)/usr/share/luci/menu.d
	$(INSTALL_DATA) ./root/usr/share/luci/menu.d/luci-app-natfrp.json \
		$(1)/usr/share/luci/menu.d/luci-app-natfrp.json

	$(INSTALL_DIR) $(1)/usr/share/rpcd/acl.d
	$(INSTALL_DATA) ./root/usr/share/rpcd/acl.d/luci-app-natfrp.json \
		$(1)/usr/share/rpcd/acl.d/luci-app-natfrp.json

	# uci 配置与 procd 脚本
	$(INSTALL_DIR) $(1)/etc/config
	$(INSTALL_CONF) ./root/etc/config/natfrp $(1)/etc/config/natfrp

	$(INSTALL_DIR) $(1)/etc/init.d
	$(INSTALL_BIN) ./root/etc/init.d/natfrp $(1)/etc/init.d/natfrp
endef

$(eval $(call BuildPackage,luci-app-natfrp))
