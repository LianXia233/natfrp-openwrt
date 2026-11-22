#
# luci-app-natfrp —— SakuraFrp 内网穿透的 LuCI 页面
#
# 只装架构无关内容：JS 页面、菜单、ACL、uci 配置、init 脚本。
# 不依赖 luci.mk / luci feed，SDK 里可直接编译。
#
include $(TOPDIR)/rules.mk

PKG_NAME:=luci-app-natfrp
PKG_VERSION:=3.2.0
PKG_RELEASE:=1

PKG_LICENSE:=MIT
PKGARCH:=all

include $(INCLUDE_DIR)/package.mk

define Package/luci-app-natfrp
  SECTION:=luci
  CATEGORY:=LuCI
  SUBMENU:=3. Applications
  TITLE:=SakuraFrp Launcher
  URL:=https://www.natfrp.com/
  DEPENDS:=+natfrp-launcher +natfrpctl
  PKGARCH:=all
endef

define Package/luci-app-natfrp/description
 SakuraFrp 内网穿透的 LuCI 页面：状态展示、配置同步、WebUI 与远程管理入口。
endef

define Package/luci-app-natfrp/conffiles
/etc/config/natfrp
endef

define Package/luci-app-natfrp/install
	$(INSTALL_DIR) $(1)/www/luci-static/resources/view/natfrp
	$(INSTALL_DATA) ./htdocs/luci-static/resources/view/natfrp/main.js \
		$(1)/www/luci-static/resources/view/natfrp/main.js

	$(INSTALL_DIR) $(1)/usr/share/luci/menu.d
	$(INSTALL_DATA) ./root/usr/share/luci/menu.d/luci-app-natfrp.json \
		$(1)/usr/share/luci/menu.d/luci-app-natfrp.json

	$(INSTALL_DIR) $(1)/usr/share/rpcd/acl.d
	$(INSTALL_DATA) ./root/usr/share/rpcd/acl.d/luci-app-natfrp.json \
		$(1)/usr/share/rpcd/acl.d/luci-app-natfrp.json

	$(INSTALL_DIR) $(1)/etc/config
	$(INSTALL_CONF) ./root/etc/config/natfrp $(1)/etc/config/natfrp

	$(INSTALL_DIR) $(1)/etc/init.d
	$(INSTALL_BIN) ./root/etc/init.d/natfrp $(1)/etc/init.d/natfrp
endef

$(eval $(call BuildPackage,luci-app-natfrp))
