<p float="left">
  <picture><source srcset="./icons/64x64/io.github.aganzha.Stage.png"><img valign="middle" alt="Stage logo" src="./icons/64x64/io.github.aganzha.Stage.png" width="32"></picture>
   <strong>Stage</strong> -
   <span>Git GUI client for linux desktops inspired by Magit</span>
</p>

## Installing
```sh
flatpak install -u https://github.com/aganzha/stage/raw/master/stage.flatpakref
```

## Running
```sh
flatpak run io.github.aganzha.Stage
```
## Using
To create commit, it need "to stage" `Unstagd changes` either all of them or separate files/hunks.
> [!NOTE]
> You can use either buttons in headerbar or press `command keys` on your keyboard which will apply to line highlighted by cursor 

<picture><source srcset="https://github.com/user-attachments/assets/aae0b833-6979-4644-8f4c-83f4eda739c1"><img alt="Stage screenshot" src="https://github.com/user-attachments/assets/aae0b833-6979-4644-8f4c-83f4eda739c1"></picture>

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
- **Push** hit `P` or press <picture><img valign="middle" alt="Push button" src="./icons/send-to-symbolic.svg" width="12"></picture> button

### Branches
Hit `B` in Status view or press <picture><source srcset="./icons/org.gtk.gtk4.NodeEditor-symbolic.svg" > <img valign="middle" alt="Branches button" src="./icons/org.gtk.gtk4.NodeEditor-symbolic.svg"></picture> button to open Branches view.
<picture><source srcset="https://github.com/user-attachments/assets/a07cd1bf-b435-40ad-beca-edbabc5d285f"> <img alt="Branches view" src="https://github.com/user-attachments/assets/a07cd1bf-b435-40ad-beca-edbabc5d285f"></picture>

#### Resolving conflicts
When in conflict hit `S` to either Theirs or Others side of conflict to choose change to be applied.

### Log
Pressing `L` opens log view or <picture><source srcset="./icons/org.gnome.Logs-symbolic.svg"><img valign="middle" alt="Push button" src="./icons/org.gnome.Logs-symbolic.svg" width="12"></picture> opens **Git log view**

### Cherry-pick/Revert
Both actions are available on all views (Branches, Logs, Commit and Stash views) by pressing respectivelly `A` (apply) `R` (revert) 
<picture><source srcset="./icons/emblem-shared-symbolic.svg"><img valign="middle" alt="Apply button" src="./icons/emblem-shared-symbolic.svg" width="12"></picture> <picture><source srcset="./icons/edit-undo-symbolic.svg"><img valign="middle" alt="Revert button" src="./icons/edit-undo-symbolic.svg" width="12"></picture> buttons.

### Stash
Pressing `Z` or <picture><source srcset="./icons/sidebar-show-symbolic.svg"><img valign="middle" alt="Push button" src="./icons/sidebar-show-symbolic.svg" width="12"></picture> opens **Stashes panel**
<picture><source srcset="https://github.com/user-attachments/assets/22f7b87d-42fd-4358-b719-5ea705df1f41"><img alt="Stashes view" src="https://github.com/user-attachments/assets/22f7b87d-42fd-4358-b719-5ea705df1f41"></picture>

### Tags
Pressing `T` opens Tags view
