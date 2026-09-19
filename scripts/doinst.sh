config() {
  NEW="$1"; OLD="$(dirname $NEW)/$(basename $NEW .new)"
  if [ ! -r $OLD ]; then
    mv $NEW $OLD
  elif [ "$(cat $OLD | md5sum)" = "$(cat $NEW | md5sum)" ]; then
    rm $NEW
  fi
}
config etc/slacker/slacker.conf.new
config etc/slacker/mirrors.new
config etc/slacker/repos.new
config etc/slacker/blacklist.new
config etc/slacker/distro-upgrade.conf.new
config etc/slacker/credentials.cat.new

# Refresh the icon and desktop caches after installing slacker-gui.
if [ -e usr/share/icons/hicolor/icon-theme.cache ]; then
  if [ -x /usr/bin/gtk-update-icon-cache ]; then
    /usr/bin/gtk-update-icon-cache -f usr/share/icons/hicolor >/dev/null 2>&1
  fi
fi

if [ -x /usr/bin/update-desktop-database ]; then
  /usr/bin/update-desktop-database -q usr/share/applications >/dev/null 2>&1
fi


if [ -d etc/slacker/credentials.d ]; then
  chmod 0700 etc/slacker/credentials.d
fi
if [ -e etc/slacker/credentials.cat ]; then
  chmod 0600 etc/slacker/credentials.cat
fi
