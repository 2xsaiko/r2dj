use std::cmp::min;
use std::io;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use dasp::ring_buffer::Bounded;
use dasp::{Frame, Signal};
use dasp_graph::{process, BoxedNodeSend, Buffer, Input, NodeData};
use futures::Sink;
use log::warn;
use petgraph::graph::NodeIndex;
use petgraph::Direction;

use crate::streamio::StreamWrite;

// Choose a type of graph for audio processing.
type Graph = petgraph::graph::DiGraph<NodeData<Node>, (), u32>;
// Create a short-hand for our processor type.
type Processor = dasp_graph::Processor<Graph>;

#[derive(Debug)]
enum Node {
    NoOp,
    Input { node: BoxedNodeSend, channels: u8 },
    Output { node: BoxedNodeSend, channels: u8 },
    Boxed(BoxedNodeSend),
}

impl dasp_graph::Node for Node {
    fn process(&mut self, inputs: &[Input], output: &mut [Buffer]) {
        match self {
            Node::NoOp => {}
            Node::Input { node, .. } => node.process(inputs, output),
            Node::Output { node, .. } => node.process(inputs, output),
            Node::Boxed(n) => n.process(inputs, output),
        }
    }
}

struct CoreData {
    graph: Graph,
    processor: Processor,
    bottom: NodeIndex,
    default_output: Option<NodeIndex>,
}

impl CoreData {
    fn new() -> Self {
        let mut graph = Graph::with_capacity(10, 10);
        let processor = Processor::with_capacity(10);

        let bottom = graph.add_node(NodeData::new(Node::NoOp, vec![Buffer::SILENT; 0]));

        CoreData {
            graph,
            processor,
            bottom,
            default_output: None,
        }
    }

    fn add_node<N>(&mut self, node: NodeData<N>) -> NodeIndex
    where
        N: dasp_graph::Node + Send + 'static,
    {
        self.graph.add_node(NodeData::new(
            Node::Boxed(BoxedNodeSend::new(node.node)),
            node.buffers,
        ))
    }

    fn add_input_to(&mut self, output: Option<NodeIndex>) -> AudioSource {
        let shared = Arc::new(AudioSourceShared {
            running: AtomicBool::new(false),
            data: Mutex::new(AudioSourceShared1 {
                buffer: Bounded::from(vec![[0.0; 2]; 512]),
                write_waker: None,
            }),
        });

        let node = self.graph.add_node(NodeData::new(
            Node::Input {
                node: BoxedNodeSend::new(InputNode {
                    shared: shared.clone(),
                }),
                channels: 2u8,
            },
            vec![Buffer::default(); 2],
        ));

        if let Some(output) = output {
            self.graph.add_edge(node, output, ());
        }

        AudioSource { shared, node }
    }

    fn add_output(&mut self) -> OutputSignal {
        let shared = Arc::new(Mutex::new(OutputNodeShared {
            buffer: Bounded::from(vec![[0.0; 2]; 8192]),
        }));

        let node = self.graph.add_node(NodeData::new(
            Node::Output {
                node: BoxedNodeSend::new(OutputNode {
                    shared: shared.clone(),
                }),
                channels: 2u8,
            },
            vec![Buffer::default(); 2],
        ));

        self.graph.add_edge(node, self.bottom, ());

        if self.default_output.is_none() {
            self.default_output = Some(node);
        }

        OutputSignal { shared, node }
    }

    fn tick(&mut self) {
        process(&mut self.processor, &mut self.graph, self.bottom);
    }

    fn sinks(&self) -> impl Iterator<Item = NodeIndex> + '_ {
        self.graph
            .neighbors_directed(self.bottom, Direction::Incoming)
    }
}

#[derive(Clone)]
pub struct Core {
    data: Arc<Mutex<CoreData>>,
    sample_rate: u32,
}

impl Core {
    pub fn new(sample_rate: u32) -> Self {
        let data = Arc::new(Mutex::new(CoreData::new()));
        let c = Core { data, sample_rate };
        tokio::spawn(c.clone().run());
        c
    }

    pub fn add_input(&self) -> AudioSource {
        let mut data = self.data.lock().unwrap();
        let out = data.default_output;
        data.add_input_to(out)
    }

    pub fn add_input_to(&self, output: Option<NodeIndex>) -> AudioSource {
        self.data.lock().unwrap().add_input_to(output)
    }

    pub fn add_output(&self) -> OutputSignal {
        self.data.lock().unwrap().add_output()
    }

    async fn run(self) {
        let mut interval = tokio::time::interval(Duration::from_secs_f64(
            Buffer::LEN as f64 / self.sample_rate as f64,
        ));
        // let buffer_rate = self.sample_rate as usize / Buffer::LEN;

        loop {
            interval.tick().await;
            let mut data = self.data.lock().unwrap();
            data.tick();
        }
    }
}

type SampleBuffer = Bounded<Vec<[f32; 2]>>;

#[derive(Debug)]
struct AudioSourceShared {
    running: AtomicBool,
    data: Mutex<AudioSourceShared1>,
}

#[derive(Debug)]
struct AudioSourceShared1 {
    buffer: SampleBuffer,
    write_waker: Option<Waker>,
}

#[derive(Debug)]
pub struct AudioSource {
    shared: Arc<AudioSourceShared>,
    node: NodeIndex,
}

impl AudioSource {
    pub fn set_running(&self, running: bool) {
        self.shared.running.store(running, Ordering::Relaxed);
    }

    pub fn running(&self) -> bool {
        self.shared.running.load(Ordering::Relaxed)
    }

    pub fn push(&self, sample: [f32; 2]) -> Option<[f32; 2]> {
        let mut data = self.shared.data.lock().unwrap();
        data.buffer.push(sample)
    }

    pub fn node(&self) -> NodeIndex {
        self.node
    }
}

impl StreamWrite<[f32; 2]> for AudioSource {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[[f32; 2]],
    ) -> Poll<io::Result<usize>> {
        let mut data = self.shared.data.lock().unwrap();

        if data.buffer.is_full() {
            data.write_waker = Some(cx.waker().clone());
            Poll::Pending
        } else {
            let to_write = min(data.buffer.max_len() - data.buffer.len(), buf.len());

            for el in &buf[..to_write] {
                data.buffer.push(*el);
            }

            Poll::Ready(Ok(to_write))
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

impl Sink<[f32; 2]> for AudioSource {
    type Error = ();

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let mut data = self.shared.data.lock().unwrap();

        if data.buffer.is_full() {
            data.write_waker = Some(cx.waker().clone());
            Poll::Pending
        } else {
            Poll::Ready(Ok(()))
        }
    }

    fn start_send(self: Pin<&mut Self>, item: [f32; 2]) -> Result<(), Self::Error> {
        let mut data = self.shared.data.lock().unwrap();

        data.buffer.push(item);

        Ok(())
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }
}

struct InputNode {
    shared: Arc<AudioSourceShared>,
}

impl dasp_graph::Node for InputNode {
    fn process(&mut self, _inputs: &[Input], output: &mut [Buffer]) {
        if self.shared.running.load(Ordering::Relaxed) {
            let mut data = self.shared.data.lock().unwrap();
            let mut underflow = 0;

            for i in 0..Buffer::LEN {
                let sample = match data.buffer.pop() {
                    None => {
                        underflow += 1;
                        [0.0; 2]
                    }
                    Some(s) => s,
                };

                for ch in 0..2 {
                    output[ch][i] = sample[ch];
                }
            }

            if underflow > 0 {
                warn!("buffer underflow: {} samples missing", underflow);
            }

            if let Some(waker) = data.write_waker.take() {
                waker.wake();
            }
        } else {
            output.iter_mut().for_each(|b| b.silence());
        }
    }
}

#[derive(Debug)]
struct OutputNodeShared {
    buffer: Bounded<Vec<[f32; 2]>>,
}

struct OutputNode {
    shared: Arc<Mutex<OutputNodeShared>>,
}

#[derive(Debug)]
pub struct OutputSignal {
    shared: Arc<Mutex<OutputNodeShared>>,
    node: NodeIndex,
}

impl dasp_graph::Node for OutputNode {
    fn process(&mut self, inputs: &[Input], _output: &mut [Buffer]) {
        let mut shared = self.shared.lock().unwrap();

        let mut output = [[0.0; 2]; Buffer::LEN];

        for input in inputs.iter() {
            assert_eq!(2, input.buffers().len());

            for (ch, buffer) in input.buffers().iter().enumerate() {
                for (idx, sample) in buffer.iter().enumerate() {
                    output[idx][ch] += *sample;
                }
            }
        }

        for el in output.iter() {
            shared.buffer.push(*el);
        }
    }
}

impl Signal for OutputSignal
where
    [f32; 2]: Frame,
{
    type Frame = [f32; 2];

    fn next(&mut self) -> Self::Frame {
        let mut shared = self.shared.lock().unwrap();
        shared.buffer.pop().unwrap_or(Frame::EQUILIBRIUM)
    }
}

impl OutputSignal {
    pub fn node(&self) -> NodeIndex {
        self.node
    }
}

// fn nodedata_map<F, T, U>(node: NodeData<T>, op: F) -> NodeData<U>
// where
//     F: FnOnce(T) -> U,
// {
//     NodeData::new(op(node.node), node.buffers)
// }
