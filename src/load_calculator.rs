use evalexpr::*;

pub struct LoadCalculator {
    node: Node,
    context: HashMapContext,
}

impl LoadCalculator {
    pub fn new(formula: &str) -> anyhow::Result<Self> {
        let node = build_operator_tree::<DefaultNumericTypes>(formula)?;

        Ok(Self {
            node,
            context: HashMapContext::new(),
        })
    }

    pub fn set_float_var(&mut self, name: &str, value: f64) -> anyhow::Result<()> {
        self.context.set_value(name.to_string(), Value::Float(value))?;
        Ok(())
    }

    pub fn set_int_var(&mut self, name: &str, value: i64) -> anyhow::Result<()> {
        self.context.set_value(name.to_string(), Value::Int(value))?;
        Ok(())
    }

    pub fn calculate(&self) -> anyhow::Result<f32> {
        Ok(self.node.eval_float_with_context(&self.context)? as f32)
    }
}
