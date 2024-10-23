<p float="left">
   <img valign="middle" alt="Stage logo" src="./icons/64x64/io.github.aganzha.Stage.png" width="32">
   <strong>Stage</strong> -
   <span>Git GUI client for linux desktops inspired by Magit</span>
</p>

## Installing
```sh
flatpak install -u https://github.com/aganzha/stage/raw/master/stage.flatpakref
```

## Using
```sh
flatpak run io.github.aganzha.Stage
```
> [!NOTE]
> Stage will watch your repository for changes and display them as diff for commit

<table width="100%">
  <tr>
    <td align="center"><strong>Gedit</strong></td>
    <td align="center"><strong>Stage</strong></td>
  </tr>
</table>

[![Stage demo](https://www.aganzha.online/demo3.mp4)


### Staging

- **Expand/collapse** underlying files and hunks by pressing `TAB` or `SPACE` or `clicking` expandable items on screen.

- **Stage** selected files or hunks or all changes by pressing `ENTER` or `S` or `double clicking` items on screen

- **Unstage** staged changes by pressing `U` or `double clicking` while cursor is on staged items.

- **Kill** unstaged changes by pressing `K` while cursor is on unstaged items.


### Commit/Push/Pull
- **Commit** hit `C` or press <span><img valign="middle" alt="Commit button" src="./icons/object-select-symbolic.svg" width="12"/></span> button
- **Pull** hit `F` (fetch) or press <span><img valign="middle" alt="Pull button" src="./icons/document-save-symbolic.svg" width="12"/></span> button
- **Push** hit `P` or press <span><img valign="middle" alt="Push button" src="./icons/send-to-symbolic.svg" width="12"/></span> button

### Stash
Pressing `Z` or <span><img valign="middle" alt="Push button" src="./icons/sidebar-show-symbolic.svg" width="12"/></span> opens **Stashes panel**
### Log
Pressing `L` opens log view or <span><img valign="middle" alt="Push button" src="./icons/org.gnome.Logs-symbolic.svg" width="12"/></span> opens **Git log view**
### Cherry-pick/Revert
### Branches
##### Resolving conflicts
