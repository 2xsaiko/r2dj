import React, { Component } from 'react';
import { hot } from 'react-hot-loader';
import { DefaultPalette, INavLink, INavLinkGroup, IStackStyles, Nav, Stack } from '@fluentui/react';
import './App.css';
import PlaylistManager from './PlaylistManager';
import TabList, { TabData } from './widgets/TabList';

const stackStyles: IStackStyles = {
    root: {
        background: DefaultPalette.themePrimary,
        width: '100%',
        height: '100%',
    },
};

const navLinkGroups: INavLinkGroup[] = [
    {
        links: [
            {
                name: 'Home',
                key: 'home',
                url: '',
            },
            {
                name: 'Playlists',
                key: 'playlists',
                url: '',
            },
        ],
    },
];

interface Props {}

interface State {
    currentItem: INavLink;
    currentTab: TabData;
}

class App extends Component<Props, State> {
    constructor(props: Props) {
        super(props);
        this.state = {
            currentItem: navLinkGroups[0].links[0],
            currentTab: tabs[0],
        };
    }

    render() {
        return (
            <div className="App">
                <Stack verticalFill>
                    <Stack.Item>
                        <Stack horizontal styles={stackStyles}>
                            foo
                        </Stack>
                    </Stack.Item>
                    <Stack.Item grow>
                        <Stack horizontal>
                            <Stack.Item disableShrink styles={{ root: { width: 200 } }}>
                                <Nav
                                    groups={navLinkGroups}
                                    selectedKey={this.state.currentItem.key}
                                    onLinkClick={(ev, item) => this.navigateTo(ev, item)}
                                />
                            </Stack.Item>
                            <Stack.Item grow styles={{ root: { minWidth: 0 } }}>
                                <TabList
                                    tabs={tabs}
                                    selected={this.state.currentTab.key}
                                    onActivate={(tab) => this.selectTab(tab)}
                                />
                                <PlaylistManager />
                            </Stack.Item>
                        </Stack>
                    </Stack.Item>
                    <Stack.Item>
                        <Stack horizontal styles={stackStyles}>
                            baz
                        </Stack>
                    </Stack.Item>
                </Stack>
            </div>
        );
    }

    private navigateTo(ev?: React.MouseEvent<HTMLElement>, item?: INavLink) {
        if (item) {
            this.setState({ currentItem: item });
        }
    }

    private selectTab(tab: TabData) {
        this.setState({
            currentTab: tab,
        });
    }
}

const tabs: TabData[] = (() => {
    let list = [];

    for (let i = 0; i < 20; i += 1) {
        list.push({
            key: `${i}`,
            title: `Demo Tab ${i}`,
        });
    }

    return list;
})();

export default hot(module)(App);
