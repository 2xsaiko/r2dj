import {Component} from "react";
import {CommandBar, DetailsList, Dropdown, IColumn, ICommandBarItemProps, IDropdownOption, IStackTokens, SelectionMode, Stack, TextField} from "@fluentui/react";

const columns: IColumn[] = [
    {
        key: 'icon',
        name: 'Icon',
        minWidth: 16,
        maxWidth: 16,
    },
    {
        key: 'name',
        name: 'Name',
        minWidth: 16,
        isResizable: true,
    },
    {
        key: 'artist',
        name: 'Artist',
        minWidth: 16,
        isResizable: true,
    },
    {
        key: 'album',
        name: 'Album',
        minWidth: 16,
        isResizable: true,
    },
    {
        key: 'added',
        name: 'Added',
        minWidth: 16,
        isResizable: true,
    }
];

const syncType: IDropdownOption[] = [
    {key: 'none', text: 'None'},
    {key: 'youtube', text: 'YouTube'},
    {key: 'spotify', text: 'Spotify'},
];

const headerStackTokens: IStackTokens = {
    childrenGap: 10,
    padding: 10,
};

class PlaylistManager extends Component {
    render() {
        return (
            <div>
                <CommandBar items={_items}/>
                <Stack horizontal tokens={headerStackTokens}>
                    <Stack.Item grow>
                        <TextField label="Playlist"/>
                        <TextField label="Name"/>
                    </Stack.Item>
                    <Stack.Item grow>
                        <Dropdown options={syncType} label='External Source' defaultSelectedKey='none'/>
                        <TextField label='External Source URL' disabled/>
                    </Stack.Item>
                </Stack>
                <DetailsList
                    items={[]}
                    columns={columns}
                    selectionMode={SelectionMode.multiple}
                />
            </div>
        );
    }
}

const _items: ICommandBarItemProps[] = [
    {
        key: 'new',
        text: 'New',
        iconProps: {iconName: 'Add'},
        split: true,
        ariaLabel: 'New',
        subMenuProps: {
            items: [
                {key: 'duplicate', text: 'Duplicate', iconProps: {iconName: 'Copy'}},
            ],
        },
    },
    {
        key: 'save',
        text: 'Save',
        iconProps: {iconName: 'Save'},
        ariaLabel: 'Save'
    },
    {
        key: 'delete',
        text: 'Delete',
        iconProps: {iconName: 'Delete'},
        ariaLabel: 'Delete'
    },
    {
        key: 'sync',
        text: 'Sync',
        title: 'Synchronize playlist with remote source',
        iconProps: {iconName: 'Download'}
    }
];

export default PlaylistManager;