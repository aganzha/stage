<p float="left">
  <picture><source srcset="./icons/64x64/io.github.aganzha.Stage.png"><img valign="middle" alt="Stage logo" src="./icons/64x64/io.github.aganzha.Stage.png" width="32"></picture>
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

![Stage Screenshot](https://github.com/user-attachments/assets/22f7b87d-42fd-4358-b719-5ea705df1f41)

> [!NOTE]
> While running Stage will watch your repository for changes and display them as diff for your commit


<div align="center">
<table width="600">
  <tr>
    <td align="center">Editing in Gedit&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;</td>                                                                  
    <td align="center">&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;Reflected in Stage</td> 
  </tr>
</table>
</div>

https://github.com/user-attachments/assets/d3fe0575-7a0a-444c-938a-40af9e75bceb


### Staging

- **Expand/collapse** underlying files and hunks by pressing `TAB` or `SPACE` or `clicking` expandable items on screen.

- **Stage** selected files or hunks or all changes by pressing `ENTER` or `S` or `double clicking` items on screen

- **Unstage** staged changes by pressing `U` or `double clicking` while cursor is on staged items.

- **Kill** unstaged changes by pressing `K` while cursor is on unstaged items.


### Commit/Push/Pull
- **Commit** hit `C` or press <picture><source srcset="./icons/object-select-symbolic.svg"><img valign="middle" alt="Commit button" src="./icons/object-select-symbolic.svg" width="12"></picture> button
- **Pull** hit `F` (fetch) or press <picture><source srcset="./icons/document-save-symbolic.svg"><img valign="middle" alt="Pull button" src="./icons/document-save-symbolic.svg"></picture> button
- **Push** hit `P` or press <span><img valign="middle" alt="Push button" src="./icons/send-to-symbolic.svg" width="12"></span> button

### Stash
Pressing `Z` or <picture><source srcset="./icons/sidebar-show-symbolic.svg"><img valign="middle" alt="Push button" src="./icons/sidebar-show-symbolic.svg" width="12"></picture> opens **Stashes panel**
### Log
Pressing `L` opens log view or <picture><src srcset="./icons/org.gnome.Logs-symbolic.svg"><img valign="middle" alt="Push button" src="./icons/org.gnome.Logs-symbolic.svg" width="12"></picture> opens **Git log view**
### Cherry-pick/Revert
Both actions are available on all views (Branches, Logs and Commit/Stash views) by pressing respectivelly `A` (apply) / `R` (revert) <picture><src srcset="./icons/emblem-shared-symbolic.svg"><img valign="middle" alt="Push button" src="./icons/emblem-shared-symbolic.svg" width="12"></picture> / <picture><src srcset="./icons/edit-undo-symbolic.svg"><img valign="middle" alt="Push button" src="./icons/edit-undo-symbolic.svg" width="12"></picture> buttons.
### Branches
##### Resolving conflicts
