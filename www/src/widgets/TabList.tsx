import { IconButton, NeutralColors, Stack, VerticalDivider } from '@fluentui/react';
import React, { Component, CSSProperties, WheelEventHandler } from 'react';

const tabStyle: CSSProperties = {
    height: '32px',
};

const mouseOverTabStyle: CSSProperties = {
    backgroundColor: NeutralColors.gray20,
    ...tabStyle,
};

const inactiveTabStyle: CSSProperties = {
    backgroundColor: NeutralColors.gray40,
    ...tabStyle,
};

const tabTextStyle: CSSProperties = {
    margin: '0 10px',
    userSelect: 'none',
    whiteSpace: 'nowrap',
};

interface TabData {
    key: string;
    title?: string;
    hideClose?: boolean;
    closeDisabled?: boolean;
}

interface BaseProps {
    tabs: TabData[];
    selected: string;
}

interface Props extends BaseProps {
    onClose?(tab: TabData): void;

    onActivate?(tab: TabData): void;
}

interface RawTabListProps extends Props {
    onWheel?: WheelEventHandler<HTMLElement>;
    mouseOverTab?: string;

    onMouseEnter(tab: TabData): void;

    onMouseLeave(tab: TabData): void;
}

interface State {
    isAtLeft: boolean;
    isAtRight: boolean;
    mouseOverTab?: string;
}

interface TabProps {
    tab: TabData;
    active: boolean;
    mouseOver: boolean;

    onClose(): void;

    onActivate(): void;

    onMouseEnter(): void;

    onMouseLeave(): void;
}

function Tab(props: TabProps) {
    let style = props.active ? tabStyle : props.mouseOver ? mouseOverTabStyle : inactiveTabStyle;

    return (
        <Stack.Item grow style={style}>
            <Stack
                horizontal
                styles={{ root: { height: '100%' } }}
                verticalAlign="center"
                onMouseDown={() => props.onActivate()}
                onMouseEnter={() => props.onMouseEnter()}
                onMouseLeave={() => props.onMouseLeave()}
            >
                <Stack.Item grow style={tabTextStyle}>
                    {props.tab.title}
                </Stack.Item>
                {props.tab.hideClose ? (
                    []
                ) : (
                    <Stack.Item>
                        <IconButton
                            iconProps={{ iconName: 'ChromeClose' }}
                            disabled={props.tab.closeDisabled}
                            onClick={() => props.onClose()}
                        />
                    </Stack.Item>
                )}
            </Stack>
        </Stack.Item>
    );
}

function RawTabList(props: RawTabListProps) {
    let elements = [];

    for (let tab of props.tabs) {
        if (elements.length > 0) {
            elements.push(
                <Stack.Item>
                    <VerticalDivider />
                </Stack.Item>,
            );
        }

        elements.push(
            <Tab
                tab={tab}
                active={props.selected == tab.key}
                mouseOver={props.mouseOverTab == tab.key}
                onActivate={() => props.onActivate?.(tab)}
                onClose={() => props.onClose?.(tab)}
                onMouseEnter={() => props.onMouseEnter(tab)}
                onMouseLeave={() => props.onMouseLeave(tab)}
            />,
        );
    }

    return (
        <Stack horizontal onWheel={props.onWheel}>
            {elements}
        </Stack>
    );
}

class TabList extends Component<Props, State> {
    private readonly elem: React.RefObject<HTMLDivElement>;
    private resizeObserver?: ResizeObserver;

    constructor(props: Props) {
        super(props);
        this.elem = React.createRef();
        this.state = {
            isAtLeft: true,
            isAtRight: true,
        };
    }

    componentDidMount() {
        let current = this.elem.current;

        if (current) {
            this.resizeObserver = new ResizeObserver(() => {
                this.updateScrollState();
            });
            this.resizeObserver.observe(current);
        }

        this.updateScrollState();
    }

    componentWillUnmount() {
        if (this.resizeObserver) {
            this.resizeObserver.disconnect();
        }
    }

    render() {
        return (
            <Stack horizontal styles={{ root: { width: '100%' } }}>
                <Stack.Item grow styles={{ root: { flexBasis: 0, overflow: 'auto' } }}>
                    <div ref={this.elem} style={{ overflow: 'hidden' }}>
                        <RawTabList
                            mouseOverTab={this.state.mouseOverTab}
                            onWheel={(event) => this.scroll(event)}
                            {...this.props}
                            onMouseEnter={(tab) => this.tabMouseOver(tab)}
                            onMouseLeave={(tab) => this.tabMouseOverEnd(tab)}
                        />
                    </div>
                </Stack.Item>
                <Stack.Item>
                    <IconButton
                        title="Scroll Left"
                        iconProps={{ iconName: 'CaretLeftSolid8' }}
                        disabled={this.state.isAtLeft}
                        onClick={() => this.scrollLeft()}
                    />
                </Stack.Item>
                <Stack.Item>
                    <IconButton
                        title="Scroll Right"
                        iconProps={{ iconName: 'CaretRightSolid8' }}
                        disabled={this.state.isAtRight}
                        onClick={() => this.scrollRight()}
                    />
                </Stack.Item>
            </Stack>
        );
    }

    private scroll(event: React.WheelEvent<HTMLElement>) {
        let current = this.elem.current;

        if (current) {
            current.scrollLeft += (event.deltaX + event.deltaY) * 0.5;
        }

        this.updateScrollState();
    }

    private updateScrollState() {
        this.setState({
            isAtLeft: this.isAtStart(),
            isAtRight: this.isAtEnd(),
        });
    }

    private isAtStart(): boolean {
        let current = this.elem.current;

        if (current) {
            return current.scrollLeft <= 0;
        } else {
            return true;
        }
    }

    private isAtEnd(): boolean {
        let current = this.elem.current;

        if (current) {
            return current.scrollLeft >= current.scrollWidth - current.clientWidth;
        } else {
            return true;
        }
    }

    private scrollLeft() {
        let current = this.elem.current;

        if (current) {
            current.scrollLeft -= 100;
        }

        this.updateScrollState();
    }

    private scrollRight() {
        let current = this.elem.current;

        if (current) {
            current.scrollLeft += 100;
        }

        this.updateScrollState();
    }

    private tabMouseOver(tab: TabData) {
        this.setState({
            mouseOverTab: tab.key,
        });
    }

    private tabMouseOverEnd(tab: TabData) {
        if (this.state.mouseOverTab == tab.key) {
            this.setState({
                mouseOverTab: undefined,
            });
        }
    }
}

export default TabList;
export type { TabData };