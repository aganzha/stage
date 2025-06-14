<p float="left">
  <picture><source srcset="./icons/64x64/io.github.aganzha.Stage.png"><img valign="middle" alt="Stage logo" src="./icons/64x64/io.github.aganzha.Stage.png" width="32"></picture>
   <strong>Stage</strong> -
   <span>Git GUI client for linux desktops inspired by Magit</span>
</p>

![CI Build/Tests](https://github.com/aganzha/stage/actions/workflows/tests.yml/badge.svg)
[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](https://www.gnu.org/licenses/gpl-3.0)
[![Copr build status](https://copr.fedorainfracloud.org/coprs/aganzha/stage/package/stage-git-gui/status_image/last_build.png)](https://copr.fedorainfracloud.org/coprs/aganzha/stage/package/stage-git-gui/)
[![Docs](https://img.shields.io/badge/docs-orange)](https://aganzha.github.io/stage/)

## Installing
### Flatpak
Add [Flathub](https://flathub.org/apps/io.github.aganzha.Stage) to your remotes
```sh
flatpak remote-add --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
flatpak install flathub io.github.aganzha.Stage
flatpak run io.github.aganzha.Stage
```

### Fedora 42
```sh
sudo dnf install copr
sudo dnf copr enable aganzha/stage
sudo dnf install stage-git-gui
stage-git-gui
```
### Ubuntu 25.04
```sh
sudo add-apt-repository ppa:aganzha/stage
sudo apt update
sudo apt install stage-git-gui
stage-git-gui
```

## Using
Open repository either by clicking placeholder button, or repository list button in the header bar. Stage, then, will live track all changes you do on files inside repository and display in Status window in form of text diff.

> [!NOTE]
> Highlighted line in Status window behave like a cursor in TUI apps.

Move cursor around with arrows or by mouse clicking in any area. Commands your issued for Stage are applied to "thing" under cursor. E.g. to stage (git add) file put cursor on file name and press `s`, or click <img class="inline" src="https://raw.githubusercontent.com/keenlycode/gnomicon/refs/heads/main/src/icon/go-bottom-symbolic.svg"/> in header bar. Whole file will be added to staging area for further commit.

- Use `Ctrl +` / `Ctrl -` to change font size.
- dark / light theme switcher is in the burger

<picture><source srcset="https://github.com/user-attachments/assets/aae0b833-6979-4644-8f4c-83f4eda739c1"><img alt="Stage screenshot" src="https://github.com/user-attachments/assets/aae0b833-6979-4644-8f4c-83f4eda739c1"></picture>

### Main commands

- `s` - **S**tage selected files or hunks or all changes by pressing `enter`. <img class="inline" src="https://raw.githubusercontent.com/keenlycode/gnomicon/refs/heads/main/src/icon/go-bottom-symbolic.svg"/>
- `u` - **U**nstage. Button - <img class="inline" src="https://raw.githubusercontent.com/keenlycode/gnomicon/refs/heads/main/src/icon/go-top-symbolic.svg"/></li>
- `k` - **K**ill
- `Tab/Space` - Expand/collapse underlying files and hunks.

> [!NOTE]
> Stage operates on files and hunks as native git. You can expand/collapse files and hunks to view changes and choose hunks for commit.

+ When cursor is on file name, **whole file** is a subject to issued command.
+ When cursor is on hunk header or any line inside hunk, then **current hunk** is subject to command

> [!NOTE]
> Current hunk under cursor is slightly highlighted.

### Commit/Push/Pull
- `c` - **C**ommit. Button in headerbar - <picture><source srcset="./icons/object-select-symbolic.svg"><img valign="middle" alt="Commit button" src="./icons/object-select-symbolic.svg" width="12"></picture>
- `f` - Pull (as in **F**etch). Button - <picture><source srcset="./icons/document-save-symbolic.svg"><img valign="middle" alt="Pull button" src="./icons/document-save-symbolic.svg"></picture>
- `p` - **P**ush. Button - <picture><img valign="middle" alt="Push button" src="./icons/send-to-symbolic.svg" width="12"></picture>

### Command showing other windows
- `b` - Branches window <picture><source srcset="./icons/org.gtk.gtk4.NodeEditor-symbolic.svg" > <img valign="middle" alt="Branches button" src="./icons/org.gtk.gtk4.NodeEditor-symbolic.svg"></picture>
- `l` - opens log window <picture><source srcset="./icons/org.gnome.Logs-symbolic.svg"><img valign="middle" alt="Push button" src="./icons/org.gnome.Logs-symbolic.svg" width="12"></picture>
- `z` - opens stashes panel <picture><source srcset="./icons/sidebar-show-symbolic.svg"><img valign="middle" alt="Push button" src="./icons/sidebar-show-symbolic.svg" width="12"></picture>
- `t` - opens tags window

### Other Commands
- `o` - opens quick repo selector
- `Ctrl` + `o` - opens repo choosing dialog
- `Ctrl` + `b` - blame line under cursor


> [!NOTE]
> Any window above Status window could be closed with `Esc` or `Ctrl-w`

### Branches window
<picture><source srcset="https://github.com/user-attachments/assets/a07cd1bf-b435-40ad-beca-edbabc5d285f"> <img alt="Branches view" src="https://github.com/user-attachments/assets/a07cd1bf-b435-40ad-beca-edbabc5d285f"></picture>
Current branch is marked with <img class="inline" src="https://raw.githubusercontent.com/keenlycode/gnomicon/refs/heads/main/src/icon/avatar-default-symbolic.svg"/> icon

#### Switching, creating and deleting branches
This window allows quickly switch between branches: just move cursor with arrows and hit <code>enter</code>, or double click.

To create new branch hit <code>c</code> or <img class="inline" src="https://raw.githubusercontent.com/keenlycode/gnomicon/refs/heads/main/src/icon/list-add-symbolic.svg"/> button.<br/>      
> [!TIP]
> The branch you are creating will be based on the branch on which the cursor currently is. This means you can create new branch from branch `feature` even though the current branch is `master`, and quickly switch to it.

To delete branch hit `k` or <img class="inline" src="https://raw.githubusercontent.com/keenlycode/gnomicon/refs/heads/main/src/icon/user-trash-symbolic.svg"/> button
> [!WARNING]
> There are no any confirmation for branch deleting

#### Merge and rebase
Put cursor on branch you want to merge in current (<img class="inline" src="https://raw.githubusercontent.com/keenlycode/gnomicon/refs/heads/main/src/icon/avatar-default-symbolic.svg"/>) branch and hit `m` (<img class="inline" src="https://raw.githubusercontent.com/keenlycode/gnomicon/refs/heads/main/src/icon/media-playlist-shuffle-symbolic.svg"/>). Use `r` (<img class="inline" src="https://raw.githubusercontent.com/keenlycode/gnomicon/refs/heads/main/src/icon/media-playlist-repeat-song-symbolic.svg"/>) for rebase.

> [!NOTE]
> Sooner or later you will have conflicts during merge/rebase. When Stage displays conflicts it behaves a bit differently: when cursor is on `ours` or `theirs` side of conflict, whole side is highlighted and hitting `s`tage will resolve this conflict. Conflict will disapear from **Conflicts** section. Sometimes you will see final result in **Staged** section, but it could not be the case if after resolving there are no changes in source code (e.g. you choose `ours` side and source code remains the same).

#### View branch commits
Hit `l` (as in **L**og) to view commits in branch under cursor in Log window.

### Remotes
Remote branches are just separate section in branches list and their behaviour and commands are just the same as local branches. E.g. just hit `enter` or double click on remote branch and Stage will fetch it and switch to it.

To update remote branches hit <code>u</code> or press <img class="inline" src="https://raw.githubusercontent.com/keenlycode/gnomicon/refs/heads/main/src/icon/view-refresh-symbolic.svg"/> button in headerbar.

> [!TIP]
> You can manage remotes in Status window by pressing <img class="inline" src="https://raw.githubusercontent.com/keenlycode/gnomicon/refs/heads/main/src/icon/network-server-symbolic.svg"/> button.

#### Log window
When in main Status window or in Branches window hitting `l` (<img class="inline" src="https://raw.githubusercontent.com/aganzha/stage/refs/heads/master/icons/org.gnome.Logs-symbolic.svg"/>) will bring up Log window.      
  
> [!NOTE]
> Stage does not display merge commits

Log window is just a list of commits. You can search among them via panel in headerbar. Commits which come from other branches displaying arrows in separate column for convinience

#### Commits window
Hitting `enter` or single click on commit sha in Log window brings up the commit content window. Individual Commit window behaves same way as Status window, except its readonly.

- Hit `a` (as in **A**pply) to Cherry-pick commit onto current branch <img class="inline" src="https://raw.githubusercontent.com/keenlycode/gnomicon/refs/heads/main/src/icon/emblem-shared-symbolic.svg"/>
- Hit `r` (as in **R**evert) to Revert commit onto current branch <img class="inline" src="https://raw.githubusercontent.com/keenlycode/gnomicon/refs/heads/main/src/icon/edit-undo-symbolic.svg"/></li>
  
### Stashes panel
Hitting `z` or <img class="inline" src="https://raw.githubusercontent.com/keenlycode/gnomicon/refs/heads/main/src/icon/sidebar-show-symbolic.svg"/> icon will open stashes panel. Hitting <code>z</code> one more time will stash all changes.

### Tags window
Hitting `t` in Status window brings up Tags window. That window behave as a simple list where you can `c` - create, `k` - delete (as in **K**ill) and `p` - to push tags to remote.

### Blame
Git blame in Stage is a bit strange :smiley: Stage do not want to read your files directly. It only operates on diffs produced by libgit2. So, to view history of some line in code this line must somehow apear in Stage. This means you have to edit or delete this line :smiley:. Or line nearby (each change in git surrounded by 3 lines of context above and below). When you see your line in Stage you can put cursor on it and hit `Ctrl`+`b`. This will open up commit window pointing this line origin. Again, this works in Commit window to: hitting any line (except green one) in Commit window will bring another window with commit which contains this line adding.
