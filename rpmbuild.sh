#!/bin/bash
original_name="stage"
name="stage-git-gui"
version="0.1.18"
release=1
full_id="io.github.aganzha.Stage"
spec_name="stage-git-gui.spec"
rm -rf ~/rpmbuild/
rpmdev-setuptree

# rename binary
sed -i "s/name = \"$original_name\"/name = \"$name\"/" Cargo.toml
sed -i "s|Exec=$original_name|Exec=$name|g" $full_id.desktop

# create archive
tar_name="$name"-"$version".x86_64.tar.gz

# base spec
tar czvf ~/rpmbuild/SOURCES/$tar_name --exclude-vcs --exclude=target --exclude='*~' --exclude='#*#' --transform="s|^|${name}-${version}/|" .
rust2rpm --path $(pwd)/Cargo.toml -t fedora $name@$version

# release
sed -i "s/^Release:.*$/Release:        $release%{?dist}/" $spec_name

# fixmes
sed -i 's|URL:            # FIXME|URL:            https:://github.com/aganzha/stage|' $spec_name
sed -i '/^Source:         # FIXME/a %global out_dir .' $spec_name
sed -i "s|Source:         # FIXME|Source:         $tar_name|" $spec_name
sed -i "s|License:        # FIXME|License:        GPL-3.0-or-later|" $spec_name

# xvfb for tests
sed -i '/^BuildRequires:  cargo-rpm-macros >= 26/a BuildRequires: xorg-x11-server-Xvfb' $spec_name
sed -i 's/%cargo_test/xvfb-run bash -c '"'"'%cargo_test'"'"'/g' $spec_name

# update desktop database
sed -i '/^BuildRequires:  cargo-rpm-macros >= 26/a Requires(post): desktop-file-utils' $spec_name

# env on build
sed -i '/^%build/a export OUT_DIR=%{out_dir}' $spec_name
sed -i '/^%build/a glib-compile-resources $(pwd)/io.github.aganzha.Stage.gresource.xml --target $(pwd)/src/gresources.compiled' $spec_name

# env on install
sed -i '/^%install/a export OUT_DIR=%{out_dir}' $spec_name

# env on check
sed -i '/^%check/a export OUT_DIR=%{out_dir}' $spec_name

# after cargo_install --------- upside down (last directives will be first in spec because of sed 'after')
# icons
sed -i "/^%cargo_install/a install -m 644 icons/$full_id.svg %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/$full_id.svg" $spec_name
sed -i "/^%cargo_install/a install -m 644 icons/512x512/$full_id.png %{buildroot}%{_datadir}/icons/hicolor/512x512/apps/$full_id.png" $spec_name
sed -i "/^%cargo_install/a install -m 644 icons/256x256/$full_id.png %{buildroot}%{_datadir}/icons/hicolor/256x256/apps/$full_id.png" $spec_name
sed -i "/^%cargo_install/a install -m 644 icons/128x128/$full_id.png %{buildroot}%{_datadir}/icons/hicolor/128x128/apps/$full_id.png" $spec_name
sed -i "/^%cargo_install/a install -m 644 icons/64x64/$full_id.png %{buildroot}%{_datadir}/icons/hicolor/64x64/apps/$full_id.png" $spec_name
sed -i "/^%cargo_install/a install -m 644 icons/32x32/$full_id.png %{buildroot}%{_datadir}/icons/hicolor/32x32/apps/$full_id.png" $spec_name
sed -i "/^%cargo_install/a install -m 644 icons/16x16/$full_id.png %{buildroot}%{_datadir}/icons/hicolor/16x16/apps/$full_id.png" $spec_name
sed -i "/^%cargo_install/a install -m 644 icons/$full_id.svg %{buildroot}%{_datadir}/icons/hicolor/symbolic/apps/$full_id-symbolic.svg" $spec_name
sed -i "/^%cargo_install/a install -m 644 icons/org.gnome.Logs-symbolic.svg %{buildroot}%{_datadir}/icons/hicolor/symbolic/apps/org.gnome.Logs-symbolic.svg" $spec_name

# meta
sed -i "/^%cargo_install/a install -m 644 $full_id.desktop %{buildroot}%{_datadir}/applications/$full_id.desktop" $spec_name
sed -i "/^%cargo_install/a install -m 644 $full_id.metainfo.xml %{buildroot}%{_datadir}/metainfo/$full_id.metainfo.xml" $spec_name
sed -i "/^%cargo_install/a install -m 644 $full_id.gschema.xml %{buildroot}%{_datadir}/glib-2.0/schemas/$full_id.gschema.xml" $spec_name

# icons
sed -i '/^%cargo_install/a mkdir -p %{buildroot}%{_datadir}/icons/hicolor/scalable/apps' $spec_name
sed -i '/^%cargo_install/a mkdir -p %{buildroot}%{_datadir}/icons/hicolor/512x512/apps' $spec_name
sed -i '/^%cargo_install/a mkdir -p %{buildroot}%{_datadir}/icons/hicolor/256x256/apps' $spec_name
sed -i '/^%cargo_install/a mkdir -p %{buildroot}%{_datadir}/icons/hicolor/128x128/apps' $spec_name
sed -i '/^%cargo_install/a mkdir -p %{buildroot}%{_datadir}/icons/hicolor/64x64/apps' $spec_name
sed -i '/^%cargo_install/a mkdir -p %{buildroot}%{_datadir}/icons/hicolor/32x32/apps' $spec_name
sed -i '/^%cargo_install/a mkdir -p %{buildroot}%{_datadir}/icons/hicolor/16x16/apps' $spec_name
sed -i '/^%cargo_install/a mkdir -p %{buildroot}%{_datadir}/icons/hicolor/symbolic/apps' $spec_name


# meta
sed -i '/^%cargo_install/a mkdir -p %{buildroot}%{_datadir}/applications' $spec_name
sed -i '/^%cargo_install/a mkdir -p %{buildroot}%{_datadir}/metainfo' $spec_name
sed -i '/^%cargo_install/a mkdir -p %{buildroot}%{_datadir}/glib-2.0/schemas' $spec_name

# declare files before changelog
# icons
sed -i "/^%changelog/i %{_datadir}/icons/hicolor/scalable/apps/$full_id.svg" $spec_name
sed -i "/^%changelog/i %{_datadir}/icons/hicolor/512x512/apps/$full_id.png" $spec_name
sed -i "/^%changelog/i %{_datadir}/icons/hicolor/256x256/apps/$full_id.png" $spec_name
sed -i "/^%changelog/i %{_datadir}/icons/hicolor/128x128/apps/$full_id.png" $spec_name
sed -i "/^%changelog/i %{_datadir}/icons/hicolor/64x64/apps/$full_id.png" $spec_name
sed -i "/^%changelog/i %{_datadir}/icons/hicolor/32x32/apps/$full_id.png" $spec_name
sed -i "/^%changelog/i %{_datadir}/icons/hicolor/16x16/apps/$full_id.png" $spec_name
sed -i "/^%changelog/i %{_datadir}/icons/hicolor/symbolic/apps/$full_id-symbolic.svg" $spec_name
sed -i "/^%changelog/i %{_datadir}/icons/hicolor/symbolic/apps/org.gnome.Logs-symbolic.svg" $spec_name

# meta
sed -i "/^%changelog/i %{_datadir}/applications/$full_id.desktop" $spec_name
sed -i "/^%changelog/i %{_datadir}/metainfo/$full_id.metainfo.xml" $spec_name
sed -i "/^%changelog/i %{_datadir}/glib-2.0/schemas/$full_id.gschema.xml" $spec_name

# adding post directive to compile schema, update caches etc
sed -i '/^%changelog/i\
%post\
update-desktop-database &> /dev/null || :\
if [ -x %{_bindir}/gtk-update-icon-cache ]; then\
    %{_bindir}/gtk-update-icon-cache --quiet %{_datadir}/icons/hicolor || :\
fi\
%postun\
if [ "$1" = "0" ]; then\
    /usr/bin/update-desktop-database -q /usr/share/applications &>/dev/null || :\
fi' $spec_name

# building
mv $spec_name ~/rpmbuild/SPECS/
git checkout io.github.aganzha.Stage.desktop
git checkout Cargo.toml
git checkout Cargo.lock
rpmbuild -bs ~/rpmbuild/SPECS/$spec_name
# toolbox run -c f42-rpmbuild rpmbuild -ba ~/rpmbuild/SPECS/$spec_name
# copr-cli build aganzha/stage ~/rpmbuild/SRPMS/stage-git-gui-0.1.18-1.fc42.src.rpm
