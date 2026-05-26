use anyhow::bail;
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

    pub fn update_formula(&mut self, formula: &str) -> anyhow::Result<()> {
        self.node = build_operator_tree::<DefaultNumericTypes>(formula)?;
        Ok(())
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
        let val = self.node.eval_with_context(&self.context)?;

        Ok(match val {
            Value::Boolean(b) => {
                if b {
                    1.0
                } else {
                    0.0
                }
            }
            Value::Int(val) => val as f32,
            Value::Float(val) => val as f32,
            _ => bail!(
                "load formula returned {val:?}, when a boolean, integer or a float was expected"
            ),
        })
    }
}
