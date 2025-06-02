#!/bin/bash
name="stage-git-gui"
version="0.1.18"
full_id="io.github.aganzha.Stage"
spec_name="stage-git-gui.spec"
rm -rf ~/rpmbuild/
rpmdev-setuptree
sed -i "s/name = \"color\"/name = \"$name\"/" Cargo.toml
tar_name="$name"-"$version".x86_64.tar.gz
# tar czvf ~/rpmbuild/SOURCES/$tar_name --exclude-vcs --exclude=target --exclude='*~' --exclude='#*#' .
this_dir=$(basename "$PWD")
tar czvf ~/rpmbuild/SOURCES/$tar_name --exclude-vcs --exclude=target --exclude='*~' --exclude='#*#' --transform="s|^|${name}-${version}/|" .
rust2rpm --path $(pwd)/Cargo.toml -t fedora $name@$version
sed -i 's|URL:            # FIXME|URL:            https:://github.com/aganzha/stage|' $spec_name
sed -i '/^Source:         # FIXME/a %global out_dir .' $spec_name
sed -i "s|Source:         # FIXME|Source:         $tar_name|" $spec_name
sed -i '/^%build/a export OUT_DIR=%{out_dir}' $spec_name
sed -i '/^%build/a glib-compile-resources $(pwd)/src/io.github.aganzha.Stage.gresource.xml --target $(pwd)/src/gresources.compiled' $spec_name
sed -i '/^%install/a export OUT_DIR=%{out_dir}' $spec_name
sed -i '/^%check/a export OUT_DIR=%{out_dir}' $spec_name

sed -i "/^%cargo_install/a install -m 644 $full_id.desktop %{buildroot}%{_datadir}/applications/$full_id.desktop" $spec_name
sed -i "/^%cargo_install/a install -m 644 $full_id.svg %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/$full_id.svg" $spec_name
sed -i "/^%cargo_install/a install -m 644 $full_id.metainfo.xml %{buildroot}%{_datadir}/metainfo/$full_id.metainfo.xml" $spec_name

sed -i '/^%cargo_install/a mkdir -p %{buildroot}%{_datadir}/applications' $spec_name
sed -i '/^%cargo_install/a mkdir -p %{buildroot}%{_datadir}/icons/hicolor/scalable/apps' $spec_name
sed -i '/^%cargo_install/a mkdir -p %{buildroot}%{_datadir}/metainfo' $spec_name

sed -i "/^%changelog/i %{_datadir}/applications/$full_id.desktop" $spec_name
sed -i "/^%changelog/i %{_datadir}/icons/hicolor/scalable/apps/$full_id.svg" $spec_name
sed -i "/^%changelog/i %{_datadir}/metainfo/$full_id.metainfo.xml" $spec_name
exit 0
mv $spec_name ~/rpmbuild/SPECS/
rpmbuild -bs ~/rpmbuild/SPECS/$spec_name
# toolbox run -c f42-rpmbuild rpmbuild -ba ~/rpmbuild/SPECS/$spec_name
