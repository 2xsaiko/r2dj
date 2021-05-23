use std::cmp::min;
use std::io;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use dasp::interpolate::linear::Linear;
use dasp::ring_buffer::Bounded;
use dasp::sample::Duplex;
use dasp::signal::interpolate::Converter;
use dasp::{Frame, Signal};
use dasp_graph::{process, BoxedNode, BoxedNodeSend, Buffer, Input, NodeData};
use log::warn;
use petgraph::graph::NodeIndex;
use petgraph::Direction;

use crate::streamio::StreamWrite;
use futures::Sink;

pub struct Tap<S> {
    running: bool,
    signal: S,
}

impl<S> Signal for Tap<S>
where
    S: Signal,
{
    type Frame = S::Frame;

    fn next(&mut self) -> Self::Frame {
        if self.running {
            self.signal.next()
        } else {
            Self::Frame::EQUILIBRIUM
        }
    }

    fn is_exhausted(&self) -> bool {
        self.signal.is_exhausted()
    }
}

impl<S> Tap<S> {
    pub fn into_inner(self) -> S {
        self.signal
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    pub fn set_running(&mut self, running: bool) {
        self.running = running;
    }
}

pub struct Limiter<S> {
    signal: S,
    rate: u32,
}

impl<S, T> Limiter<S>
where
    S: Signal,
    S::Frame: Frame<Sample = T>,
    T: Duplex<f64>,
{
    pub fn resample(mut self, rate: u32) -> Limiter<Converter<S, Linear<S::Frame>>> {
        let s1 = self.signal.next();
        let s2 = self.signal.next();
        Limiter {
            signal: self
                .signal
                .from_hz_to_hz(Linear::new(s1, s2), self.rate as f64, rate as f64),
            rate,
        }
    }
}

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

    fn add_input_to<const CHANNELS: usize>(
        &mut self,
        output: Option<NodeIndex>,
    ) -> AudioSource<CHANNELS> {
        let shared = Arc::new(AudioSourceShared {
            running: AtomicBool::new(false),
            data: Mutex::new(AudioSourceShared1 {
                buffer: Bounded::from(vec![[0.0; CHANNELS]; 512]),
                write_waker: None,
            }),
        });

        let node = self.graph.add_node(NodeData::new(
            Node::Input {
                node: BoxedNodeSend::new(InputNode {
                    shared: shared.clone(),
                }),
                channels: CHANNELS as u8,
            },
            vec![Buffer::default(); CHANNELS],
        ));

        if let Some(output) = output {
            self.graph.add_edge(node, output, ());
        }

        AudioSource { shared, node }
    }

    fn add_output<const CHANNELS: usize>(&mut self) -> OutputSignal<CHANNELS> {
        let shared = Arc::new(Mutex::new(OutputNodeShared {
            buffer: Bounded::from(vec![[0.0; CHANNELS]; 8192]),
        }));

        let node = self.graph.add_node(NodeData::new(
            Node::Output {
                node: BoxedNodeSend::new(OutputNode {
                    shared: shared.clone(),
                }),
                channels: CHANNELS as u8,
            },
            vec![Buffer::default(); CHANNELS],
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
        let c = Core {
            data,
            sample_rate,
        };
        tokio::spawn(c.clone().run());
        c
    }

    pub fn add_input<const CHANNELS: usize>(&self) -> AudioSource<CHANNELS> {
        let mut data = self.data.lock().unwrap();
        let out = data.default_output;
        data.add_input_to(out)
    }

    pub fn add_input_to<const CHANNELS: usize>(
        &self,
        output: Option<NodeIndex>,
    ) -> AudioSource<CHANNELS> {
        self.data.lock().unwrap().add_input_to(output)
    }

    pub fn add_output<const CHANNELS: usize>(&self) -> OutputSignal<CHANNELS> {
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

#[derive(Debug)]
struct AudioSourceShared<const CHANNELS: usize> {
    running: AtomicBool,
    data: Mutex<AudioSourceShared1<CHANNELS>>,
}

#[derive(Debug)]
struct AudioSourceShared1<const CHANNELS: usize> {
    buffer: Bounded<Vec<[f32; CHANNELS]>>,
    write_waker: Option<Waker>,
}

#[derive(Debug)]
pub struct AudioSource<const CHANNELS: usize> {
    shared: Arc<AudioSourceShared<CHANNELS>>,
    node: NodeIndex,
}

impl<const CHANNELS: usize> AudioSource<CHANNELS> {
    pub fn set_running(&self, running: bool) {
        self.shared.running.store(running, Ordering::Relaxed);
    }

    pub fn running(&self) -> bool {
        self.shared.running.load(Ordering::Relaxed)
    }

    pub fn push(&self, sample: [f32; CHANNELS]) -> Option<[f32; CHANNELS]> {
        let mut data = self.shared.data.lock().unwrap();
        data.buffer.push(sample)
    }

    pub fn node(&self) -> NodeIndex {
        self.node
    }
}

impl<const CHANNELS: usize> StreamWrite<[f32; CHANNELS]> for AudioSource<CHANNELS> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[[f32; CHANNELS]],
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

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

impl<const CHANNELS: usize> Sink<[f32; CHANNELS]> for AudioSource<CHANNELS> {
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

    fn start_send(self: Pin<&mut Self>, item: [f32; CHANNELS]) -> Result<(), Self::Error> {
        let mut data = self.shared.data.lock().unwrap();

        data.buffer.push(item);

        Ok(())
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }
}

struct InputNode<const CHANNELS: usize> {
    shared: Arc<AudioSourceShared<CHANNELS>>,
}

impl<const CHANNELS: usize> dasp_graph::Node for InputNode<CHANNELS> {
    fn process(&mut self, _inputs: &[Input], output: &mut [Buffer]) {
        if self.shared.running.load(Ordering::Relaxed) {
            let mut data = self.shared.data.lock().unwrap();
            let mut underflow = 0;

            for i in 0..Buffer::LEN {
                let sample = match data.buffer.pop() {
                    None => {
                        underflow += 1;
                        [0.0; CHANNELS]
                    }
                    Some(s) => s,
                };

                for ch in 0..CHANNELS {
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
struct OutputNodeShared<const CHANNELS: usize> {
    buffer: Bounded<Vec<[f32; CHANNELS]>>,
}

struct OutputNode<const CHANNELS: usize> {
    shared: Arc<Mutex<OutputNodeShared<CHANNELS>>>,
}

#[derive(Debug)]
pub struct OutputSignal<const CHANNELS: usize> {
    shared: Arc<Mutex<OutputNodeShared<CHANNELS>>>,
    node: NodeIndex,
}

impl<const CHANNELS: usize> dasp_graph::Node for OutputNode<CHANNELS> {
    fn process(&mut self, inputs: &[Input], _output: &mut [Buffer]) {
        let mut shared = self.shared.lock().unwrap();

        let mut output = [[0.0; CHANNELS]; Buffer::LEN];

        for input in inputs.iter() {
            assert_eq!(CHANNELS, input.buffers().len());

            for (ch, buffer) in input.buffers().iter().enumerate() {
                for (idx, sample) in buffer.iter().enumerate() {
                    output[idx][ch] = *sample;
                }
            }
        }

        let mut overflow = 0;

        for el in output.iter() {
            if let Some(_) = shared.buffer.push(*el) {
                overflow += 1;
            }
        }

        if overflow > 0 {
            warn!("buffer overflow: {} samples dropped", overflow);
        }
    }
}

impl<const CHANNELS: usize> Signal for OutputSignal<CHANNELS>
where
    [f32; CHANNELS]: Frame,
{
    type Frame = [f32; CHANNELS];

    fn next(&mut self) -> Self::Frame {
        let mut shared = self.shared.lock().unwrap();
        shared.buffer.pop().unwrap_or(Frame::EQUILIBRIUM)
    }
}

impl<const CHANNELS: usize> OutputSignal<CHANNELS> {
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
