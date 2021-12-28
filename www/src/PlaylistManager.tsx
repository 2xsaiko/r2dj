import { Component, CSSProperties } from 'react';
import {
    ContextualMenuItemType,
    DetailsList,
    DetailsListLayoutMode,
    Dropdown,
    IColumn,
    ICommandBarItemProps,
    IDropdownOption,
    IStackTokens,
    IStyle,
    SelectionMode,
    Stack,
    TextField,
    VerticalDivider,
} from '@fluentui/react';
import StandardCommandBar from './widgets/StandardCommandBar';

const columns: IColumn[] = [
    {
        key: 'icon',
        name: '',
        iconName: 'Media',
        minWidth: 16,
        maxWidth: 16,
    },
    {
        key: 'name',
        name: 'Name',
        minWidth: 210,
        maxWidth: 350,
        isResizable: true,
        isCollapsible: true,
    },
    {
        key: 'artist',
        name: 'Artist',
        minWidth: 70,
        maxWidth: 90,
        isResizable: true,
        isCollapsible: true,
    },
    {
        key: 'album',
        name: 'Album',
        minWidth: 70,
        maxWidth: 90,
        isResizable: true,
        isCollapsible: true,
    },
    {
        key: 'added',
        name: 'Added',
        minWidth: 10,
        maxWidth: 10,
        isResizable: true,
        isCollapsible: true,
    },
];

const syncTypes: IDropdownOption[] = [
    { key: 'none', text: 'None' },
    { key: 'youtube', text: 'YouTube' },
    { key: 'spotify', text: 'Spotify' },
];

const headerStackTokens: IStackTokens = {
    childrenGap: 10,
    padding: 10,
};

const fillAreaStyle: CSSProperties & IStyle = {
    height: '100%',
    width: '100%',
};

interface Props {}

interface State {
    syncType: IDropdownOption;
}

class PlaylistManager extends Component<Props, State> {
    constructor(props: Props) {
        super(props);
        this.state = {
            syncType: syncTypes[0],
        };
    }

    render() {
        return (
            <div style={fillAreaStyle}>
                <Stack verticalFill styles={{ root: fillAreaStyle }}>
                    <Stack.Item>
                        <StandardCommandBar loaded extraItems={this.items()} />
                    </Stack.Item>
                    <Stack.Item>
                        <Stack horizontal tokens={headerStackTokens}>
                            <Stack.Item grow>
                                <TextField label="Playlist" />
                                <TextField label="Name" />
                            </Stack.Item>
                            <Stack.Item grow>
                                <Dropdown
                                    options={syncTypes}
                                    label="External Source"
                                    selectedKey={this.state.syncType.key}
                                    onChange={(event, option) => this.setSyncType(option)}
                                />
                                <TextField label="External Source URL" disabled={!this.syncEnabled()} />
                            </Stack.Item>
                        </Stack>
                    </Stack.Item>
                    <Stack.Item grow>
                        <DetailsList
                            items={[]}
                            columns={columns}
                            selectionMode={SelectionMode.multiple}
                            layoutMode={DetailsListLayoutMode.justified}
                            styles={{ root: { overflow: 'hidden' } }}
                        />
                    </Stack.Item>
                </Stack>
            </div>
        );
    }

    private setSyncType(option?: IDropdownOption) {
        if (option) {
            this.setState({ syncType: option });
        }
    }

    private syncEnabled(): boolean {
        return this.state.syncType.key != 'none';
    }

    private items(): ICommandBarItemProps[] {
        return [
            {
                key: 'separator1',
                itemType: ContextualMenuItemType.Divider,
                onRender: () => <VerticalDivider />,
            },
            {
                key: 'sync',
                text: 'Sync',
                title: 'Synchronize playlist with remote source',
                iconProps: { iconName: 'Download' },
                disabled: !this.syncEnabled(),
            },
        ];
    }
}

export default PlaylistManager;
