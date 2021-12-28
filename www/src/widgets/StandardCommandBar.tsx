import { Component } from 'react';
import { CommandBar, ContextualMenuItemType, ICommandBarItemProps, VerticalDivider } from '@fluentui/react';

interface Props {
    loaded?: boolean;
    extraItems?: ICommandBarItemProps[];
}

class StandardCommandBar extends Component<Props> {
    render() {
        return <CommandBar items={this.items()} />;
    }

    private items(): ICommandBarItemProps[] {
        return [
            {
                key: 'new',
                text: 'New',
                iconProps: { iconName: 'Add' },
                split: true,
                ariaLabel: 'New',
                subMenuProps: {
                    items: [
                        {
                            key: 'duplicate',
                            text: 'Duplicate',
                            iconProps: { iconName: 'Copy' },
                            disabled: !(this.props.loaded ?? false),
                        },
                    ],
                },
            },
            {
                key: 'save',
                text: 'Save',
                iconProps: { iconName: 'Save' },
                ariaLabel: 'Save',
                disabled: !(this.props.loaded ?? false),
            },
            {
                key: 'delete',
                text: 'Delete',
                iconProps: { iconName: 'Delete' },
                ariaLabel: 'Delete',
                disabled: !(this.props.loaded ?? false),
            },
            {
                key: 'separator',
                itemType: ContextualMenuItemType.Divider,
                onRender: () => <VerticalDivider />,
            },
            {
                key: 'reload',
                text: 'Reload',
                title: 'Reload and discard changes',
                iconProps: { iconName: 'Refresh' },
            },
        ].concat(this.props.extraItems ?? []);
    }
}

export default StandardCommandBar;
export type { Props };