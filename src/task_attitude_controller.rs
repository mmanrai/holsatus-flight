
use core::f32::consts::PI;
use nalgebra::Vector3;
use pid_controller_rs::Pid;
use crate::channels;
use crate::cfg;

use defmt::*;

#[derive(Clone, Debug, Copy, PartialEq)]
pub enum StabilizationMode {
    Horizon(Vector3<f32>),
    Acro(Vector3<f32>)
}

impl Format for StabilizationMode {
    fn format(&self, fmt: Formatter) {
        defmt::write!(fmt,"{}",Debug2Format(self))
    }
}

impl StabilizationMode {
    pub fn same_variant_as(&self, rhs: &StabilizationMode) -> bool {
        self.variant() == rhs.variant()
    }

    fn variant (&self) -> usize {
        match self {
            StabilizationMode::Horizon(_) => 0,
            StabilizationMode::Acro(_) => 1,
        }
    }
}

static TASK_ID : &str = "ATTITUDE_CONTROLLER";

#[embassy_executor::task]
pub async fn attitude_controller(
    mut s_attitude_sense: channels::AttitudeSenseSub,
    mut s_attitude_int_reset : channels::AttitudeIntResetSub,
    mut s_attitude_stab_mode : channels::AttitudeStabModeSub,
    p_attitude_actuate: channels::AttitudeActuatePub,
) {

    // Aquire satbilization mode
    let mut stabilization_mode = s_attitude_stab_mode.next_message_pure().await;

    // Setup controllers for pitch, roll and yaw, using a cascaded controller scheme.
    let mut pid_pitch_outer = Pid::new( 10., 0.1, 0., true, cfg::ATTITUDE_LOOP_TIME_SECS );
    let mut pid_pitch_inner = Pid::new( 40., 1.0, 0.01, true, cfg::ATTITUDE_LOOP_TIME_SECS ).set_lp_filter(0.01);
    let mut pid_roll_outer = Pid::new( 10., 0.1, 0., true, cfg::ATTITUDE_LOOP_TIME_SECS );
    let mut pid_roll_inner = Pid::new( 30., 1.0, 0.01, true, cfg::ATTITUDE_LOOP_TIME_SECS ).set_lp_filter(0.01);
    let mut pid_yaw_outer = Pid::new( 8., 0.001, 0., true, cfg::ATTITUDE_LOOP_TIME_SECS ).set_circular(-PI, PI);
    let mut pid_yaw_inner = Pid::new( 60., 1.0, 0., true, cfg::ATTITUDE_LOOP_TIME_SECS ).set_circular(-PI, PI).set_lp_filter(0.01);

    info!("{} : Entering main loop",TASK_ID);
    loop {

        // Update reference signal (and stabilization mode) from channel
        crate::channels::update_from_channel(&mut s_attitude_stab_mode, &mut stabilization_mode );

        // Reset integrators if signaled to do so
        if let Some(true) = s_attitude_int_reset.try_next_message_pure() {
            pid_pitch_outer.reset_integral();   pid_pitch_inner.reset_integral();
            pid_roll_outer.reset_integral();    pid_roll_inner.reset_integral();
            pid_yaw_outer.reset_integral();     pid_yaw_inner.reset_integral();
        }

        // Wait for new measurements to arrive
        let (att_angle,att_rate) = s_attitude_sense.next_message_pure().await;

        // Generate actuation signal
        p_attitude_actuate.publish_immediate( match stabilization_mode {

            StabilizationMode::Horizon(reference) => {

                // Run outer part of cascaded control loop
                let outer_error = reference - att_angle;
                let inner_reference = Vector3::new(
                    pid_roll_outer.update( outer_error.x ),
                    pid_pitch_outer.update( outer_error.y ),
                    pid_yaw_outer.update( outer_error.z )
                );

                // Run inner part of cascaded control loop
                let inner_error = inner_reference - att_rate;
                Vector3::new(
                    pid_roll_inner.update( inner_error.x ),
                    pid_pitch_inner.update( inner_error.y ),
                    pid_yaw_inner.update( inner_error.z )
                )
            }

            StabilizationMode::Acro(reference) => {

                let error = reference - att_rate;
                Vector3::new(
                    pid_roll_inner.update( error.x ),
                    pid_pitch_inner.update( error.y ),
                    pid_yaw_inner.update( error.z )
                )
            }
        });
    }
}


