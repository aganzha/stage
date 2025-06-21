#!/bin/bash
# mv ~/.cargo ~/cargo.orig
# toolbox enter u2504_debbuild
# sudo apt-get update
# sudo apt-get install devscripts
# thats for building: sudo apt-get install dh-cargo debhelper cargo devscripts
# touch debian/cargo-checksum.json
# rm -rf debian/cargo_registry/
# rm -rf debian/cargo_home/
# git checkout Cargo.toml
# git checkout io.github.aganzha.Stage.desktop
# ./debbuild.sh with exit after mv $tar_name ..
# dpkg-buildpackage -us -uc - will be error about dependencies.
# copy em all and install via apt-get
# dpkg-buildpackage -us -uc again
# put proper version in debian/changelog (e.g. -2)
# https://askubuntu.com/questions/862778/how-to-overwrite-a-previously-uploaded-malformed-upstream-tarball-in-launchpads
# dh_clean
# rm debian/cargo_home/.global-cache
# git checkout Cargo.toml
# git checkout io.github.aganzha.Stage.desktop
# ./debbuild.sh with exit after mv $tar_name ..
# debuild -S -kD721B759479BF5233A2FAC54196584E65F8849A1
# cd ..
# dput -d ppa:aganzha/stage stage-git-gui_0.1.21-1_source.changes
# 
original_name="stage"
name="stage-git-gui"
version="0.1.21"
release=+ds5
# release=
full_id="io.github.aganzha.Stage"
tar_name="$name"_"$version""$release".orig.tar.xz
rm ../stage-git-gui_*
sed -i "s/name = \"$original_name\"/name = \"$name\"/" Cargo.toml
sed -i "s|Exec=$original_name|Exec=$name|g" $full_id.desktop
tar cJvf $tar_name --exclude-vcs --exclude=target --exclude='*.sh' --exclude=target --exclude='.pc' --exclude='debian' --exclude='*~' --exclude='#*#' --exclude=$tar_name .
mv $tar_name ../
debuild -S -kD721B759479BF5233A2FAC54196584E65F8849A1
git checkout Cargo.toml
git checkout io.github.aganzha.Stage.desktop
#git checkout Cargo.toml
#git checkout $full_id.desktop
# dput -d ppa:aganzha/stage stage-git-gui_0.1.21-1_source.changes 


# debuild -S
# debuild -S -kD721B759479BF5233A2FAC54196584E65F8849A1
# dpkg-buildpackage -us -uc

# cd ../
# changes_name="$name"_"$version"_amd64.changes
# debsign -k D721B759479BF5233A2FAC54196584E65F8849A1 $changes_name
# dput -d stage-git-gui_0.1.21-1_amd64.changes

# ooooooooooooooooooooold --------------------------
# apt-get install dh-cargo debhelper cargo
# changes to Cargo.toml  - put in rules.
# put proper info in debian/changelog
# dh_make -s --createorig
# dh_clean
# dpkg-buildpackage -us -uc
# debsign -k D721B759479BF5233A2FAC54196584E65F8849A1 stage-git-gui_0.1.21-1_amd64.changes
# dput stage stage-git-gui_0.1.21-1_amd64.changes
